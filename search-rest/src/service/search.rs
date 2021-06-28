use crate::{client::Client, StatusResponse};

use std::{sync::Arc, time::Duration};

use actix::Actor;
use actix_web::{http::StatusCode, web, HttpRequest, HttpResponse, Responder, ResponseError};
use log::error;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use search_index::{DocType, Index, QueryOptions};
use search_state::{HandlerStatus, IndexState, IndexStateHandler};

pub const UPDATE_INTERVAL: u64 = 5 * 60;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("The given term is too long")]
    TermTooLong,
    #[error("The given term is too short")]
    TermTooShort,
    #[error("Index error: {}", _0)]
    IndexError(#[from] search_index::Error),
    #[error("API error: {}", _0)]
    APIError(#[from] tarkov_database_rs::Error),
    #[error("State error: {}", _0)]
    StateError(#[from] search_state::Error),
}

impl ResponseError for SearchError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::TermTooShort | Self::TermTooLong => StatusCode::BAD_REQUEST,
            Self::IndexError(e) => match e {
                search_index::Error::BadQuery(_) | search_index::Error::ParseError(_) => {
                    StatusCode::BAD_REQUEST
                }
                search_index::Error::IndexError(_) | search_index::Error::UnhealthyIndex(_) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            },
            SearchError::APIError(_) | SearchError::StateError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    fn error_response(&self) -> HttpResponse {
        StatusResponse {
            message: format!("{}", self),
            code: self.status_code().as_u16(),
        }
        .into()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response<T> {
    count: usize,
    data: Vec<T>,
}

impl<T: Serialize> Responder for Response<T> {
    fn respond_to(self, _req: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::Ok().json(web::Json(self))
    }
}

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    #[serde(alias = "q")]
    query: String,
    r#type: Option<DocType>,
    kind: Option<String>,
    limit: Option<usize>,
    conjunction: Option<bool>,
}

pub struct Search;

impl Search {
    pub async fn new_state(
        client: Client,
        update_interval: Duration,
    ) -> Result<(Arc<IndexState>, Arc<HandlerStatus>), SearchError> {
        let mut client = client;

        if !client.token_is_valid() {
            client.refresh_token().await?;
        }

        let index = Arc::new(IndexState::new(Index::new()?));
        index.update_items(client.get_items_all().await?)?;

        let handler = IndexStateHandler::new(index.clone(), client, update_interval);
        let status = handler.status_ref();

        IndexStateHandler::create(|_ctx| handler);

        Ok((index, status))
    }

    pub async fn get_handler(req: HttpRequest, opts: web::Query<QueryParams>) -> impl Responder {
        let state = req.app_data::<Arc<IndexState>>().unwrap();

        let query = &opts.query;
        let r#type = opts.r#type.clone();
        let kind = opts.kind.clone();

        let options = QueryOptions {
            limit: opts.limit.unwrap_or(30),
            conjunction: opts.conjunction.unwrap_or(false),
        };

        match query.len() {
            l if l < 3 => return Err(SearchError::TermTooShort),
            l if l > 100 => return Err(SearchError::TermTooLong),
            _ => {}
        }

        match if let Some(t) = r#type {
            state
                .index
                .search_by_type(query, t, kind.as_deref(), options)
        } else {
            state.index.query_top(query, options)
        } {
            Ok(d) => Ok(Response {
                count: d.len(),
                data: d,
            }),
            Err(e) => {
                error!("Error for query \"{}\": {}", query, e);
                Err(SearchError::IndexError(e))
            }
        }
    }
}
