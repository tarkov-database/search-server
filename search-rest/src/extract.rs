use crate::{
    authentication::{AuthenticationError, TokenClaims, TokenConfig, TokenError},
    error::Error,
    model::Status,
};

use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRef, FromRequest, FromRequestParts, TypedHeader},
    http::request::Parts,
};
use headers::{authorization::Bearer, Authorization};
use hyper::Request;
use serde::de::DeserializeOwned;

/// JSON extractor with custom error response
pub struct Json<T>(pub T);

#[async_trait]
impl<S, B, T> FromRequest<S, B> for Json<T>
where
    axum::Json<T>: FromRequest<S, B, Rejection = JsonRejection>,
    S: Send + Sync,
    B: Send + 'static,
{
    type Rejection = Status;

    #[inline]
    async fn from_request(req: Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => Err(Status::new(rejection.status(), rejection.body_text())),
        }
    }
}

pub struct Query<T>(pub T);

#[async_trait]
impl<S, T> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Status;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => Err(Status::new(rejection.status(), rejection.body_text())),
        }
    }
}

pub struct TokenData<T, const VE: bool>(pub T)
where
    T: TokenClaims;

#[async_trait]
impl<S, T, const VE: bool> FromRequestParts<S> for TokenData<T, VE>
where
    TokenConfig: FromRef<S>,
    T: TokenClaims,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = TokenConfig::from_ref(state);

        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| {
                    AuthenticationError::InvalidHeader("authorization header missing".to_string())
                })?;

        let claims = T::decode(bearer.token(), &config, VE).map_err(TokenError::from)?;

        Ok(Self(claims))
    }
}
