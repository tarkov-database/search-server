use std::{env, io, process, sync::Arc, time::Duration};

use actix::Actor;
use actix_web::{http::StatusCode, web, App, HttpResponse, HttpServer, Responder};
use log::error;
use search_index::ItemIndex;
use search_state::{ClientBuilder, IndexState, IndexStateHandler};
use serde::{Deserialize, Serialize};

#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

const PORT: u16 = 8080;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

const UPDATE_INTERVAL: u64 = 15 * 60;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response<T> {
    count: usize,
    data: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse<'a> {
    message: &'a str,
    code: u16,
}

#[derive(Debug, Deserialize)]
struct Query {
    term: String,
    limit: Option<usize>,
    fuzzy: Option<bool>,
}

async fn item_query_handler(
    state: web::Data<Arc<IndexState<ItemIndex>>>,
    query: web::Query<Query>,
) -> impl Responder {
    let term = &query.term;
    let limit = query.limit.unwrap_or(30);
    let fuzzy = query.fuzzy.unwrap_or(false);

    match term.len() {
        l if l < 3 => {
            return HttpResponse::BadRequest().json(StatusResponse {
                message: "Term is too short",
                code: StatusCode::BAD_REQUEST.into(),
            })
        }
        l if l > 100 => {
            return HttpResponse::BadRequest().json(StatusResponse {
                message: "Term is too long",
                code: StatusCode::BAD_REQUEST.into(),
            })
        }
        _ => {}
    }

    match if fuzzy {
        state.index.query_top_fuzzy(term, limit)
    } else {
        state.index.query_top(term, limit)
    } {
        Ok(d) => HttpResponse::Ok().json(Response {
            count: d.len(),
            data: d,
        }),
        Err(e) => {
            error!("Query error for term \"{}\": {}", term, e);
            match e {
                search_index::Error::InvalidArgument(e) => {
                    HttpResponse::BadRequest().json(StatusResponse {
                        message: &e,
                        code: StatusCode::BAD_REQUEST.into(),
                    })
                }
                _ => HttpResponse::InternalServerError().json(StatusResponse {
                    message: &e.to_string(),
                    code: StatusCode::INTERNAL_SERVER_ERROR.into(),
                }),
            }
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

    let item_index = Arc::new(IndexState::new(match ItemIndex::new() {
        Ok(i) => i,
        Err(e) => {
            error!("Error while creating item index: {}", e);
            process::exit(2);
        }
    }));

    IndexStateHandler::create(|_ctx| {
        let host = match env::var("API_HOST") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Environment variable \"API_HOST\" is missing");
                process::exit(2);
            }
        };
        let token = match env::var("API_TOKEN") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Environment variable \"API_TOKEN\" is missing");
                process::exit(2);
            }
        };

        let client_builder = ClientBuilder::default()
            .set_token(&token)
            .set_host(&host)
            .set_user_agent(USER_AGENT);

        let client_builder = if let Ok(ca) = env::var("API_CLIENT_CA") {
            client_builder.set_ca(ca)
        } else {
            client_builder
        };

        let client_builder = if let Ok(key) = env::var("API_CLIENT_KEY") {
            let cert = match env::var("API_CLIENT_CERT") {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("Environment variable \"API_CLIENT_CERT\" is missing");
                    process::exit(2);
                }
            };
            client_builder.set_keypair(cert, key)
        } else {
            client_builder
        };

        let client = client_builder.build().unwrap();

        let update_interval = Duration::from_secs(
            env::var("UPDATE_INTERVAL")
                .unwrap_or_default()
                .parse()
                .unwrap_or(UPDATE_INTERVAL),
        );

        let mut state = IndexStateHandler::new(client, update_interval);
        state.set_item_index(item_index.clone());

        state
    });

    let server = HttpServer::new(move || {
        App::new().service(
            web::resource("/item")
                .data(item_index.clone())
                .to(item_query_handler),
        )
    })
    .bind(bind)?
    .run();

    server.await
}
