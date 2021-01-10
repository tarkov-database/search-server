use crate::{client::Client, StatusResponse};

use std::{
    future::{ready, Ready},
    sync::Arc,
    time::Duration,
};

use actix::Actor;
use actix_web::{
    dev::HttpResponseBuilder, http::StatusCode, web, HttpResponse, Responder, ResponseError,
};
use log::error;
use search_index::ItemIndex;
use search_state::{IndexState, IndexStateHandler};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const UPDATE_INTERVAL: u64 = 15 * 60;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("The given term is too long")]
    TermTooLong,
    #[error("The given term is too short")]
    TermTooShort,
    #[error("{0}")]
    IndexError(#[from] search_index::Error),
}

impl ResponseError for SearchError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::TermTooShort | Self::TermTooLong => StatusCode::BAD_REQUEST,
            Self::IndexError(e) => match e {
                search_index::Error::BadQuery(_) => StatusCode::BAD_REQUEST,
                search_index::Error::IndexError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponseBuilder::new(self.status_code()).json(StatusResponse {
            message: &format!("{}", self),
            code: self.status_code().as_u16(),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response<T> {
    count: usize,
    data: T,
}

impl<T: Serialize> Responder for Response<T> {
    type Error = actix_web::Error;
    type Future = Ready<Result<HttpResponse, actix_web::Error>>;

    fn respond_to(self, _req: &actix_web::HttpRequest) -> Self::Future {
        ready(Ok(HttpResponse::Ok().json(self)))
    }
}

#[derive(Debug, Deserialize)]
pub struct Query {
    term: String,
    limit: Option<usize>,
    fuzzy: Option<bool>,
}

pub struct Search;

impl Search {
    pub fn new_state(
        client: Client,
        update_interval: Duration,
    ) -> Result<Arc<IndexState<ItemIndex>>, SearchError> {
        let item_index = Arc::new(IndexState::new(ItemIndex::new()?));

        IndexStateHandler::create(|_ctx| {
            let mut state = IndexStateHandler::new(client, update_interval);
            state.set_item_index(item_index.clone());

            state
        });

        Ok(item_index)
    }

    pub async fn get_handler(
        state: web::Data<Arc<IndexState<ItemIndex>>>,
        query: web::Query<Query>,
    ) -> impl Responder {
        let term = &query.term;
        let limit = query.limit.unwrap_or(30);
        let fuzzy = query.fuzzy.unwrap_or(false);

        match term.len() {
            l if l < 3 => return Err(SearchError::TermTooShort),
            l if l > 100 => return Err(SearchError::TermTooLong),
            _ => {}
        }

        match if fuzzy {
            state.index.query_top_fuzzy(term, limit)
        } else {
            state.index.query_top(term, limit)
        } {
            Ok(d) => Ok(Response {
                count: d.len(),
                data: d,
            }),
            Err(e) => {
                error!("Query error for term \"{}\": {}", term, e);
                return Err(SearchError::IndexError(e));
            }
        }
    }
}
