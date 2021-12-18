mod handler;
mod routes;

use crate::{authentication::TokenClaims, error, model::Status};

use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};

pub use routes::routes;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token encoding failed")]
    Encoding,
}

impl error::ErrorResponse for TokenError {
    type Response = Status;

    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    fn error_response(&self) -> Self::Response {
        Status::new(self.status_code(), self.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Search,
    Stats,
    Token,
}

impl Default for Scope {
    fn default() -> Self {
        Self::Search
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    aud: Vec<String>,
    #[serde(with = "ts_seconds")]
    exp: DateTime<Utc>,
    #[serde(with = "ts_seconds")]
    iat: DateTime<Utc>,
    sub: String,
    scope: Vec<Scope>,
}

impl Claims {
    pub const DEFAULT_EXP_MINUTES: i64 = 60;

    pub fn new<A, S>(aud: A, sub: &str, scope: S) -> Self
    where
        A: IntoIterator<Item = String>,
        S: IntoIterator<Item = Scope>,
    {
        Self {
            aud: aud.into_iter().collect(),
            exp: Utc::now() + Duration::minutes(Self::DEFAULT_EXP_MINUTES),
            iat: Utc::now(),
            sub: sub.into(),
            scope: scope.into_iter().collect(),
        }
    }

    pub fn set_expiration(&mut self, date: DateTime<Utc>) {
        self.exp = date;
    }
}

impl TokenClaims for Claims {}
