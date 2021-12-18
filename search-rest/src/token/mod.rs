mod handler;
mod routes;

use crate::authentication::TokenClaims;

use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub use routes::routes;

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
