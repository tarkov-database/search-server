mod handler;
mod routes;

use crate::{error::ErrorResponse, model::Status};

use hyper::StatusCode;

pub use routes::routes;

#[derive(Debug, thiserror::Error)]
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

impl ErrorResponse for SearchError {
    type Response = Status;

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

    fn error_response(&self) -> Self::Response {
        Status::new(self.status_code(), self.to_string())
    }
}
