use crate::AppState;

use super::handler;

use axum::routing::get;

/// Token routes
pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/", get(handler::get).post(handler::create))
}
