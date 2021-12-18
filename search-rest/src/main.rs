mod authentication;
mod error;
mod extract;
mod health;
mod model;
mod search;
mod token;

use crate::authentication::TokenConfig;

use std::{
    env,
    iter::once,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use axum::{error_handling::HandleErrorLayer, Router, Server};
use hyper::header::AUTHORIZATION;
use search_index::Index;
use search_state::{IndexState, IndexStateHandler};
use serde::Deserialize;
use tarkov_database_rs::client::ClientBuilder;
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::broadcast::{self, Sender},
};
use tower::ServiceBuilder;
use tower_http::{
    add_extension::AddExtensionLayer,
    sensitive_headers::SetSensitiveHeadersLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};

#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub type Result<T> = std::result::Result<T, error::Error>;

const fn default_addr() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

const fn default_port() -> u16 {
    8080
}

const fn default_interval() -> Duration {
    Duration::from_secs(10 * 60)
}

#[derive(Debug, Deserialize)]
struct AppConfig {
    // HTTP server
    #[serde(default = "default_addr")]
    server_addr: IpAddr,
    #[serde(default = "default_port")]
    server_port: u16,

    // JWT
    jwt_secret: String,
    jwt_audience: Vec<String>,

    // API
    api_origin: String,
    api_token: String,
    api_client_ca: Option<PathBuf>,
    api_client_cert: Option<PathBuf>,
    api_client_key: Option<PathBuf>,

    // Search
    #[serde(default = "default_interval", with = "humantime_serde")]
    update_interval: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();

    let prefix = envy::prefixed("SEARCH_");

    let app_config: AppConfig = if dotenv::dotenv().is_ok() {
        prefix.from_iter(dotenv::vars())?
    } else {
        prefix.from_env()?
    };

    let token_config =
        TokenConfig::from_secret(app_config.jwt_secret.as_bytes(), app_config.jwt_audience);

    let client_builder = ClientBuilder::default()
        .set_origin(&app_config.api_origin)
        .set_token(&app_config.api_token)
        .set_trust_dns(false)
        .set_user_agent(USER_AGENT);

    let client_builder = if let Some(v) = app_config.api_client_ca {
        client_builder.set_ca(v)
    } else {
        client_builder
    };

    let client_builder = if let Some(cert) = app_config.api_client_cert {
        if let Some(key) = app_config.api_client_key {
            client_builder.set_keypair(cert, key)
        } else {
            return Err(error::Error::MissingConfig(
                "SEARCH_API_CLIENT_KEY".to_string(),
            ));
        }
    } else {
        client_builder
    };

    let api_client = client_builder.build().await?;

    let index = IndexState::new(Index::new()?);

    let index_handler = IndexStateHandler::new(
        index.clone(),
        api_client.clone(),
        app_config.update_interval,
    );

    let status = index_handler.status_ref();

    let shutdown_signal = get_shutdown_signal(2);

    let signal = shutdown_signal.subscribe();
    let index_handler = tokio::spawn(async move {
        index_handler.run(signal).await.unwrap();
    });

    let middleware = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(error::handle_error))
        .load_shed()
        .concurrency_limit(1024)
        .timeout(Duration::from_secs(60))
        .layer(SetSensitiveHeadersLayer::new(once(AUTHORIZATION)))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(
                    DefaultOnResponse::new()
                        .include_headers(true)
                        .latency_unit(LatencyUnit::Micros),
                ),
        )
        .layer(AddExtensionLayer::new(token_config))
        .layer(AddExtensionLayer::new(api_client))
        .layer(AddExtensionLayer::new(index))
        .layer(AddExtensionLayer::new(status));

    let svc_routes = Router::new()
        .nest("/search", search::routes())
        .nest("/token", token::routes())
        .nest("/health", health::routes());

    let routes = svc_routes.layer(middleware.into_inner());

    let addr = SocketAddr::from((app_config.server_addr, app_config.server_port));
    tracing::debug!("listening on {}", addr);
    let server = Server::bind(&addr).serve(routes.into_make_service());

    let mut signal = shutdown_signal.subscribe();
    let graceful_server = server.with_graceful_shutdown(async move {
        signal.recv().await.ok();
    });

    graceful_server.await?;
    index_handler.await?;

    Ok(())
}

fn get_shutdown_signal(rx_count: usize) -> Sender<()> {
    let (tx, _) = broadcast::channel(rx_count);

    let tx2 = tx.clone();

    tokio::spawn(async move {
        let mut sig_int = signal(SignalKind::interrupt()).unwrap();
        let mut sig_term = signal(SignalKind::terminate()).unwrap();

        tokio::select! {
            _ = sig_int.recv() => {},
            _ = sig_term.recv() => {},
        };

        tx.send(()).unwrap();
    });

    tx2
}
