use crate::{extract::TokenData, model::Response, token::Claims};

use super::{ServiceStatus, Services};

use std::sync::Arc;

use axum::extract::State;
use search_state::HandlerStatus;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    ok: bool,
    service: Services,
}

pub async fn get(
    TokenData(_claims): TokenData<Claims, true>,
    State(status): State<Arc<HandlerStatus>>,
) -> crate::Result<Response<StatusResponse>> {
    let mut ok = true;

    let index = if status.is_index_error() {
        ok = false;
        ServiceStatus::Failure
    } else {
        ServiceStatus::Ok
    };

    let api = if status.is_client_error() {
        ok = false;
        ServiceStatus::Failure
    } else {
        ServiceStatus::Ok
    };

    Ok(Response::new(StatusResponse {
        ok,
        service: Services { index, api },
    }))
}
