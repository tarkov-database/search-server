use crate::{
    authentication::{AuthenticationError, TokenClaims, TokenConfig, TokenError},
    error::Error,
    model::Status,
};

use std::borrow::Cow;

use axum::{
    async_trait,
    extract::{
        rejection::{ContentLengthLimitRejection, JsonRejection, QueryRejection},
        Extension, FromRequest, RequestParts, TypedHeader,
    },
    response::IntoResponse,
    BoxError,
};
use headers::{authorization::Bearer, Authorization};
use hyper::StatusCode;
use serde::de::DeserializeOwned;

const CONTENT_LENGTH_LIMIT: u64 = 2048;

/// JSON extractor with content length limit and custom error response
pub struct SizedJson<T>(pub T);

#[async_trait]
impl<T, B> FromRequest<B> for SizedJson<T>
where
    T: DeserializeOwned,
    B: axum::body::HttpBody + Send,
    B::Data: Send,
    B::Error: Into<BoxError>,
{
    type Rejection = axum::response::Response;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        match ContentLengthLimit::<Json<T>, CONTENT_LENGTH_LIMIT>::from_request(req).await {
            Ok(value) => Ok(Self(value.0 .0)),
            Err(err) => Err(err),
        }
    }
}

/// JSON extractor with custom error response
pub struct Json<T>(pub T);

#[async_trait]
impl<B, T> FromRequest<B> for Json<T>
where
    T: DeserializeOwned,
    B: axum::body::HttpBody + Send,
    B::Data: Send,
    B::Error: Into<BoxError>,
{
    type Rejection = Status;

    #[inline]
    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => {
                let (status, message): (_, Cow<'_, str>) = match rejection {
                    JsonRejection::JsonDataError(err) => {
                        (StatusCode::BAD_REQUEST, err.to_string().into())
                    }
                    JsonRejection::JsonSyntaxError(err) => {
                        (StatusCode::BAD_REQUEST, err.to_string().into())
                    }
                    JsonRejection::MissingJsonContentType(err) => {
                        (StatusCode::BAD_REQUEST, err.to_string().into())
                    }
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
                };

                Err(Status::new(status, message))
            }
        }
    }
}

/// ContentLengthLimit extractor with custom error response
pub struct ContentLengthLimit<T, const N: u64>(pub T);

#[async_trait]
impl<T, B, const N: u64> FromRequest<B> for ContentLengthLimit<T, N>
where
    T: FromRequest<B>,
    T::Rejection: IntoResponse,
    B: Send,
{
    type Rejection = axum::response::Response;

    #[inline]
    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        match axum::extract::ContentLengthLimit::<T, N>::from_request(req).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => {
                let (status, message): (_, Cow<'_, str>) = match rejection {
                    ContentLengthLimitRejection::PayloadTooLarge(err) => {
                        (StatusCode::BAD_REQUEST, err.to_string().into())
                    }
                    ContentLengthLimitRejection::LengthRequired(err) => {
                        (StatusCode::BAD_REQUEST, err.to_string().into())
                    }
                    ContentLengthLimitRejection::Inner(err) => return Err(err.into_response()),
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
                };

                Err(Status::new(status, message).into_response())
            }
        }
    }
}

pub struct Query<T>(pub T);

#[async_trait]
impl<T, B> FromRequest<B> for Query<T>
where
    T: DeserializeOwned,
    B: Send,
{
    type Rejection = Status;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request(req).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => {
                let (status, message): (_, Cow<'_, str>) = match rejection {
                    QueryRejection::FailedToDeserializeQueryString(_) => {
                        (StatusCode::BAD_REQUEST, "invalid query string".into())
                    }
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
                };

                Err(Status::new(status, message))
            }
        }
    }
}

pub struct TokenData<T, const VE: bool>(pub T)
where
    T: TokenClaims;

#[async_trait]
impl<B, T, const VE: bool> FromRequest<B> for TokenData<T, VE>
where
    T: TokenClaims,
    B: Send,
{
    type Rejection = Error;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Extension(config) = Extension::<TokenConfig>::from_request(req)
            .await
            .expect("token config missing");

        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req)
                .await
                .map_err(|_| {
                    AuthenticationError::InvalidHeader("authorization header missing".to_string())
                })?;

        let claims = T::decode(bearer.token(), &config, VE).map_err(TokenError::from)?;

        Ok(Self(claims))
    }
}
