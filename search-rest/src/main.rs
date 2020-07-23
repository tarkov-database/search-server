use std::{
    env, io, process,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix::{fut::wrap_future, Actor, AsyncContext, Context};
use actix_web::{http::StatusCode, web, App, HttpResponse, HttpServer, Responder};
use chrono::{DateTime, TimeZone, Utc};
use log::{error, info};
use search_index::{tarkov_database_rs::client::Client, ItemIndex};
use serde::{Deserialize, Serialize};

#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

const PORT: u16 = 8080;

const UPDATE_INTERVAL: u64 = 15 * 60;

struct IndexStateHandler {
    client: Arc<Mutex<Client>>,
    interval: Duration,
    item_index: Arc<IndexState<ItemIndex>>,
}

struct IndexState<T> {
    index: T,
    modified: Mutex<DateTime<Utc>>,
}

impl IndexStateHandler {
    fn update_indexes(&mut self, ctx: &mut Context<Self>) {
        let client = self.client.clone();
        let item_index = self.item_index.clone();

        ctx.spawn(wrap_future(async move {
            info!("Check for index changes...");

            let mut c_client = client.lock().unwrap();

            if !c_client.token_is_valid() {
                if let Err(e) = c_client.refresh_token().await {
                    error!(
                        "Couldn't update indexes: error while refreshing API token: {}",
                        e
                    );
                    return;
                }
            }

            let item_stats = match c_client.get_item_index().await {
                Ok(i) => i,
                Err(e) => {
                    error!(
                        "Couldn't update indexes: error while getting item index: {}",
                        e
                    );
                    return;
                }
            };

            let mut c_modified = item_index.modified.lock().unwrap();

            if c_modified.ge(&item_stats.modified) {
                info!("Indexes are up to date, no update required");
                return;
            }

            info!("Indexes are out of date. Perform update...");

            let items = match c_client.get_all_items().await {
                Ok(d) => d,
                Err(e) => {
                    error!(
                        "Couldn't update indexes: error while getting items from API: {}",
                        e
                    );
                    return;
                }
            };

            if let Err(e) = item_index.index.write_index(items) {
                error!(
                    "Couldn't update indexes: error while writing item index: {}",
                    e
                );
                return;
            }

            *c_modified = item_stats.modified;

            info!("Indexes updated successfully");
        }));
    }
}

impl Actor for IndexStateHandler {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.update_indexes(ctx);
        ctx.run_interval(self.interval, Self::update_indexes);
    }
}

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

    let api_host = match env::var("API_HOST") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("Environment variable \"API_HOST\" is missing");
            process::exit(2);
        }
    };
    let api_token = match env::var("API_TOKEN") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("Environment variable \"API_TOKEN\" is missing");
            process::exit(2);
        }
    };

    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let item_index = Arc::new(IndexState {
        index: match ItemIndex::new() {
            Ok(i) => i,
            Err(e) => {
                error!("Error while creating item index: {}", e);
                process::exit(2);
            }
        },
        modified: Mutex::new(Utc.timestamp(0, 0)),
    });

    let update_interval = Duration::from_secs(
        env::var("UPDATE_INTERVAL")
            .unwrap_or_default()
            .parse()
            .unwrap_or(UPDATE_INTERVAL),
    );

    IndexStateHandler::create(|_ctx| IndexStateHandler {
        client: Arc::new(Mutex::new(Client::with_host(&api_token, &api_host))),
        interval: update_interval,
        item_index: item_index.clone(),
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
