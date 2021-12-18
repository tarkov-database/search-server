use hyper::StatusCode;
use serde::{Serialize, Serializer};

#[derive(Debug)]
pub struct Response<T>(StatusCode, T)
where
    T: serde::Serialize;

impl<T> Response<T>
where
    T: serde::Serialize,
{
    const DEFAULT_STATUS: StatusCode = StatusCode::OK;

    pub fn new(body: T) -> Self {
        Self(Self::DEFAULT_STATUS, body)
    }

    pub fn with_status(status: StatusCode, body: T) -> Self {
        Self(status, body)
    }
}

impl<T> axum::response::IntoResponse for Response<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> axum::response::Response {
        let mut res = axum::Json(&self.1).into_response();
        *res.status_mut() = self.0;

        res
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(serialize_with = "se_status_code_as_u16")]
    pub code: StatusCode,
    pub message: String,
}

impl Status {
    pub fn new<S>(code: StatusCode, message: S) -> Self
    where
        S: ToString,
    {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

impl axum::response::IntoResponse for Status {
    fn into_response(self) -> axum::response::Response {
        let mut res = axum::Json(&self).into_response();
        *res.status_mut() = self.code;

        res
    }
}

fn se_status_code_as_u16<S>(x: &StatusCode, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_u16(x.as_u16())
}
