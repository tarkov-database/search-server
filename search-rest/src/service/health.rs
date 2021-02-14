use crate::StatusResponse;

use std::sync::Arc;

use actix_web::{http::StatusCode, web, HttpResponse, Responder, ResponseError};
use log::error;

use search_state::IndexState;
use serde::Serialize;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HealthError {
    #[error("Index error: {}", _0)]
    IndexError(#[from] search_index::Error),
    #[error("API error: {}", _0)]
    APIError(#[from] tarkov_database_rs::Error),
}

impl ResponseError for HealthError {
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
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
struct Response {
    ok: bool,
    service: Services,
}

impl Responder for Response {
    fn respond_to(self, _req: &actix_web::HttpRequest) -> HttpResponse {
        if self.ok {
            HttpResponse::Ok().json(web::Json(self))
        } else {
            HttpResponse::InternalServerError().json(web::Json(self))
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Services {
    index: ServiceStatus,
}

#[derive(Debug, Clone)]
enum ServiceStatus {
    Ok,
    Warning,
    Failure,
}

impl ServiceStatus {
    fn value(&self) -> u8 {
        match self {
            ServiceStatus::Ok => 0,
            ServiceStatus::Warning => 1,
            ServiceStatus::Failure => 2,
        }
    }
}

impl Serialize for ServiceStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.value())
    }
}

pub struct Health;

impl Health {
    pub async fn get_handler(state: web::Data<Arc<IndexState>>) -> impl Responder {
        let mut ok = true;

        let index_health = match state.index.check_health() {
            Ok(_) => ServiceStatus::Ok,
            Err(e) => {
                error!("Error while checking index health state: {}", e);
                ok = false;
                ServiceStatus::Failure
            }
        };

        Response {
            ok,
            service: Services {
                index: index_health,
            },
        }
    }
}
