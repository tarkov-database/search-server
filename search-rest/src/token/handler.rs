use crate::{
    authentication::{AuthenticationError, TokenClaims, TokenConfig},
    extract::{SizedJson, TokenData},
    model::Response,
};

use super::{Claims, Scope};

use std::time;

use axum::extract::Extension;
use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use tarkov_database_rs::{client::Client, model::user::User};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    token: String,
    #[serde(with = "ts_seconds")]
    expires_at: DateTime<Utc>,
}

pub async fn get(
    TokenData(mut claims): TokenData<Claims, false>,
    Extension(mut client): Extension<Client>,
    Extension(config): Extension<TokenConfig>,
) -> crate::Result<Response<TokenResponse>> {
    let user = get_user(&claims.sub, &mut client).await?;

    if user.locked {
        return Err(AuthenticationError::LockedUser.into());
    }

    claims.set_expiration(Utc::now() + Duration::minutes(Claims::DEFAULT_EXP_MINUTES));

    let token = claims.encode(&config)?;

    let response = TokenResponse {
        token,
        expires_at: claims.exp,
    };

    Ok(Response::with_status(StatusCode::CREATED, response))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    sub: String,
    scope: Vec<Scope>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    valid_for: Option<time::Duration>,
}

pub async fn create(
    TokenData(_claims): TokenData<Claims, true>,
    SizedJson(body): SizedJson<CreateRequest>,
    Extension(mut client): Extension<Client>,
    Extension(config): Extension<TokenConfig>,
) -> crate::Result<Response<TokenResponse>> {
    let user = get_user(&body.sub, &mut client).await?;

    if user.locked {
        return Err(AuthenticationError::LockedUser.into());
    }

    let audience = config.validation.aud.clone().unwrap();
    let mut claims = Claims::new(audience, &body.sub, body.scope);

    if let Some(d) = body.valid_for {
        if let Ok(d) = Duration::from_std(d) {
            claims.set_expiration(claims.iat + d);
        }
    }

    let token = claims.encode(&config)?;

    let response = TokenResponse {
        token,
        expires_at: claims.exp,
    };

    Ok(Response::with_status(StatusCode::CREATED, response))
}

async fn get_user(user_id: &str, client: &mut Client) -> crate::Result<User> {
    if !client.token_is_valid().await {
        client.refresh_token().await?;
    }

    let user = match client.get_user_by_id(user_id).await {
        Ok(u) => u,
        Err(e) => match e {
            tarkov_database_rs::Error::ResourceNotFound => {
                return Err(AuthenticationError::UnknownUser.into())
            }
            _ => return Err(e.into()),
        },
    };

    Ok(user)
}
