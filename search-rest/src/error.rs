use crate::{
    authentication::{AuthenticationError, TokenError},
    model::Status,
    search,
};

use hyper::StatusCode;
use tower::BoxError;
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing config variable: {0}")]
    MissingConfig(String),
    #[error("search index error: {0}")]
    Index(#[from] search_index::Error),
    #[error("search error: {0}")]
    Search(#[from] search::SearchError),
    #[error("authentication error: {0}")]
    Authentiaction(#[from] AuthenticationError),
    #[error("action error: {0}")]
    Token(#[from] TokenError),
    #[error("API lib error: {0}")]
    ApiLibrary(#[from] tarkov_database_rs::Error),
    #[error("Envy error: {0}")]
    Envy(#[from] envy::Error),
    #[error("hyper error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("task error: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let res = match self {
            Error::Search(e) => e.error_response(),
            Error::Hyper(e) => {
                error!("hyper error: {:?}", e);
                Status::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
            Error::Envy(_) => unreachable!(),
            Error::MissingConfig(_) => unreachable!(),
            Error::ApiLibrary(_) => unreachable!(),
            Error::Index(_) => unreachable!(),
            Error::Authentiaction(e) => e.error_response(),
            Error::Token(e) => e.error_response(),
            Error::Task(_) => unreachable!(),
        };

        res.into_response()
    }
}

pub async fn handle_error(error: BoxError) -> Status {
    if error.is::<tower::timeout::error::Elapsed>() {
        return Status::new(StatusCode::REQUEST_TIMEOUT, "request timed out");
    }

    if error.is::<tower::load_shed::error::Overloaded>() {
        return Status::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "service is overloaded, try again later",
        );
    }

    error!("internal error: {:?}", error);
    Status::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

pub trait ErrorResponse
where
    Self: std::error::Error,
{
    type Response: axum::response::IntoResponse;

    fn status_code(&self) -> axum::http::StatusCode;

    fn error_response(&self) -> Self::Response;
}
