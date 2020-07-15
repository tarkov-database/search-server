use std::{env, io, process, sync::Arc};

use actix_web::{dev::HttpResponseBuilder, http::StatusCode, web, App, HttpServer, Responder};
use search_index::{tarkov_database_rs::client::Client, ItemIndex};
use serde::{Deserialize, Serialize};

#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

const PORT: &str = "8080";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response<T> {
    count: usize,
    data: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse<'a> {
    error: &'a str,
    code: u16,
}

#[derive(Debug, Deserialize)]
struct Query {
    term: String,
    limit: usize,
}

async fn item_query_handler(
    index: web::Data<Arc<ItemIndex>>,
    query: web::Query<Query>,
) -> impl Responder {
    match index.query_top(&query.term, query.limit) {
        Ok(d) => HttpResponseBuilder::new(StatusCode::OK).json(Response {
            count: d.len(),
            data: d,
        }),
        Err(e) => HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse {
            error: &e.to_string(),
            code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        }),
    }
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let port = env::var("PORT").unwrap_or(PORT.to_string());
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

    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let mut api_client = Client::with_host(&api_token, &api_host);

    if !api_client.token_is_valid() {
        if let Err(e) = api_client.refresh_token().await {
            eprintln!("Error while refreshing API token: {}", e);
            process::exit(2);
        }
    }

    let items = match api_client.get_all_items().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error while getting items from API: {}", e);
            process::exit(2);
        }
    };

    let item_index = Arc::new(match ItemIndex::new() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error while creating item index: {}", e);
            process::exit(2);
        }
    });
    if let Err(e) = item_index.write_index(items) {
        eprintln!("Error while writing item index: {}", e);
        process::exit(2);
    }

    let factory = move || {
        App::new().service(
            web::resource("/item")
                .data(item_index.clone())
                .to(item_query_handler),
        )
    };

    let server = HttpServer::new(factory).bind(bind)?;

    server.run().await
}
