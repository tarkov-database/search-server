use std::{env, io, process, time::Duration};

use actix_web::{
    body::Body,
    error::InternalError,
    guard,
    http::{HeaderName, HeaderValue, StatusCode},
    web, App, BaseHttpResponse, HttpServer, ResponseError,
};
use client::ClientConfig;
use serde::Serialize;
use service::{
    auth::{Authentication, Config, Scope},
    health::Health,
    search::{Search, UPDATE_INTERVAL},
};
use thiserror::Error;
use tokio::sync::Mutex;

mod client;
mod service;

#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

const CONTENT_TYPE: &str = "Content-Type";

const PORT: u16 = 8080;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    #[error("API lib error: {0}")]
    APILibrary(#[from] tarkov_database_rs::Error),
    #[error("IO error: {0}")]
    IOError(#[from] io::Error),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    message: String,
    code: u16,
}

impl Into<BaseHttpResponse<Body>> for StatusResponse {
    fn into(self) -> BaseHttpResponse<Body> {
        let mut resp = BaseHttpResponse::new(StatusCode::from_u16(self.code).unwrap())
            .set_body(Body::from_slice(&serde_json::to_vec(&self).unwrap()));
        resp.headers_mut().insert(
            HeaderName::from_static(CONTENT_TYPE),
            HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
        );

        resp
    }
}

impl Into<actix_web::Error> for StatusResponse {
    fn into(self) -> actix_web::Error {
        InternalError::from_response("", self.into()).into()
    }
}

impl<T: ResponseError> From<T> for StatusResponse {
    fn from(err: T) -> Self {
        Self {
            message: err.to_string(),
            code: err.status_code().as_u16(),
        }
    }
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let port = env::var("PORT").unwrap_or_else(|_| PORT.to_string());
    let bind = format!("127.0.0.1:{}", port);

    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let auth_config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error while creating auth config: {}", e);
            process::exit(2);
        }
    };

    let client = match ClientConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error while creating client config: {}", e);
            process::exit(2);
        }
    };

    let update_interval = Duration::from_secs(
        env::var("UPDATE_INTERVAL")
            .unwrap_or_default()
            .parse()
            .unwrap_or(UPDATE_INTERVAL),
    );

    let (index, status) =
        match Search::new_state(client.clone().build().unwrap(), update_interval).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error while creating index: {}", e);
                process::exit(2);
            }
        };

    let server = HttpServer::new(move || {
        let client = Mutex::new(client.clone().build().unwrap());

        App::new()
            .app_data(
                web::QueryConfig::default()
                    .error_handler(|err, _| StatusResponse::from(err).into()),
            )
            .app_data(
                web::JsonConfig::default().error_handler(|err, _| StatusResponse::from(err).into()),
            )
            .app_data(auth_config.clone())
            .service(
                web::resource("/search")
                    .guard(guard::Get())
                    .data(index.clone())
                    .wrap(Authentication::with_scope(Scope::Search))
                    .to(Search::get_handler),
            )
            .service(
                web::scope("/token")
                    .app_data(client)
                    .service(
                        web::resource("")
                            .guard(guard::Get())
                            .to(Authentication::get_handler),
                    )
                    .service(
                        web::resource("")
                            .guard(guard::Post())
                            .guard(guard::Header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref()))
                            .wrap(Authentication::with_scope(Scope::Token))
                            .to(Authentication::post_handler),
                    ),
            )
            .service(
                web::resource("/health")
                    .guard(guard::Get())
                    .data(status.clone())
                    .wrap(Authentication::new())
                    .to(Health::get_handler),
            )
    });

    server.bind(bind)?.run().await
}
