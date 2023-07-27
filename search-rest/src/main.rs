mod authentication;
mod error;
mod extract;
mod health;
mod model;
mod search;
mod token;
mod utils;

use crate::{authentication::TokenConfig, error::Error};

use std::{
    env,
    io::{stdout, IsTerminal},
    iter::once,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{error_handling::HandleErrorLayer, extract::FromRef, routing::get, Router, Server};
use hyper::{header::AUTHORIZATION, server::conn::AddrIncoming};
use hyper_rustls::server::TlsAcceptor;
use search_index::Index;
use search_state::{HandlerStatus, IndexState, IndexStateHandler};
use serde::Deserialize;
use tarkov_database_rs::client::{Client, ClientBuilder};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::broadcast::{self, Sender},
};
use tower::ServiceBuilder;
use tower_http::{
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
    // Logging
    #[serde(default)]
    log_format: LogFormat,

    // HTTP server
    #[serde(default = "default_addr")]
    server_addr: IpAddr,
    #[serde(default = "default_port")]
    server_port: u16,
    #[serde(default)]
    server_tls: bool,
    server_tls_cert: Option<PathBuf>,
    server_tls_key: Option<PathBuf>,

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

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum LogFormat {
    Json,
    Pretty,
    Compact,
    #[default]
    Full,
}

#[derive(Clone)]
pub struct AppState {
    index: IndexState,
    index_status: Arc<HandlerStatus>,
    token_config: TokenConfig,
    api_client: Client,
}

impl FromRef<AppState> for IndexState {
    fn from_ref(state: &AppState) -> Self {
        state.index.clone()
    }
}

impl FromRef<AppState> for Arc<HandlerStatus> {
    fn from_ref(state: &AppState) -> Self {
        state.index_status.clone()
    }
}

impl FromRef<AppState> for TokenConfig {
    fn from_ref(state: &AppState) -> Self {
        state.token_config.clone()
    }
}

impl FromRef<AppState> for Client {
    fn from_ref(state: &AppState) -> Self {
        state.api_client.clone()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(stdout().is_terminal())
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env());

    let prefix = envy::prefixed("SEARCH_");

    let app_config: AppConfig = if dotenv::dotenv().is_ok() {
        prefix.from_iter(dotenv::vars())?
    } else {
        prefix.from_env()?
    };

    match app_config.log_format {
        LogFormat::Json => subscriber.json().init(),
        LogFormat::Pretty => subscriber.pretty().init(),
        LogFormat::Compact => subscriber.compact().init(),
        LogFormat::Full => subscriber.init(),
    };

    let token_config =
        TokenConfig::from_secret(app_config.jwt_secret.as_bytes(), app_config.jwt_audience);

    let api_client = {
        let builder = ClientBuilder::default()
            .set_origin(&app_config.api_origin)
            .set_token(&app_config.api_token)
            .set_trust_dns(false)
            .set_user_agent(USER_AGENT);

        let builder = if let Some(v) = app_config.api_client_ca {
            builder.set_ca(v)
        } else {
            builder
        };

        let builder = if let Some(cert) = app_config.api_client_cert {
            if let Some(key) = app_config.api_client_key {
                builder.set_keypair(cert, key)
            } else {
                return Err(error::Error::MissingConfigVar("SEARCH_API_CLIENT_KEY"));
            }
        } else {
            builder
        };

        builder.build().await?
    };

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

    let state = AppState {
        index,
        index_status: status,
        token_config,
        api_client,
    };

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
        );

    let svc_routes: Router<()> = Router::new()
        .nest("/search", search::routes())
        .nest("/token", token::routes())
        .nest("/health", health::routes())
        .with_state(state);

    let routes = Router::new()
        .route("/", get(|| async { env!("CARGO_PKG_VERSION") }))
        .merge(svc_routes)
        .layer(middleware.into_inner());

    let addr = SocketAddr::from((app_config.server_addr, app_config.server_port));
    let incoming = AddrIncoming::bind(&addr)?;

    let mut signal = shutdown_signal.subscribe();
    let graceful_shutdown = async move {
        signal.recv().await.ok();
    };

    if app_config.server_tls {
        let certs = {
            let path = app_config
                .server_tls_cert
                .ok_or(Error::MissingConfigVar("IDENTITY_SERVER_TLS_CERT"))?;
            let file = std::fs::read(path)?;
            utils::read_certs(&file[..])?
                .into_iter()
                .map(rustls::Certificate)
                .collect()
        };
        let key = {
            let path = app_config
                .server_tls_key
                .ok_or(Error::MissingConfigVar("IDENTITY_SERVER_TLS_KEY"))?;
            let file = std::fs::read(path)?;
            utils::read_key(&file[..]).map(rustls::PrivateKey)?
        };

        let incoming = TlsAcceptor::builder()
            .with_single_cert(certs, key)?
            .with_all_versions_alpn()
            .with_incoming(incoming);
        let server = Server::builder(incoming)
            .serve(routes.into_make_service())
            .with_graceful_shutdown(graceful_shutdown);

        tracing::info!(
            ipAddress =? addr.ip(),
            port =? addr.port(),
            "HTTPS server started"
        );

        server.await?;
    } else {
        let server = Server::builder(incoming)
            .serve(routes.into_make_service())
            .with_graceful_shutdown(graceful_shutdown);

        tracing::info!(
            ipAddress =? addr.ip(),
            port =? addr.port(),
            "HTTP server started"
        );

        server.await?;
    }

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
