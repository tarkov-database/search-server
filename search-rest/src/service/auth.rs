use crate::{Error, StatusResponse};

use std::{
    cell::RefCell,
    env,
    iter::FromIterator,
    rc::Rc,
    task::{Context, Poll},
    time,
};

use actix_web::{
    dev::{self, Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorInternalServerError,
    http::{header, HeaderMap, StatusCode},
    web, FromRequest, HttpRequest, HttpResponse, Responder, ResponseError,
};
use api::{client::Client, model::user::User};
use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use futures_util::{
    future::{ready, LocalBoxFuture, Ready},
    FutureExt,
};
use jsonwebtoken::{
    decode, encode,
    errors::{Error as JwtError, ErrorKind},
    DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};
use tarkov_database_rs as api;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Missing authorization header")]
    MissingHeader,
    #[error("Wrong authorization header")]
    WrongHeader,
    #[error("Token is expired")]
    ExpiredToken,
    #[error("Token is not yet valid")]
    ImmatureToken,
    #[error("Token is invalid")]
    InvalidToken,
    #[error("Insufficient permission")]
    InsufficientPermission,
    #[error("User is blocked")]
    LockedUser,
    #[error("User doesn't exist")]
    UnknownUser,
    #[error("API error: {}", _0)]
    APIError(#[from] api::Error),
}

impl From<JwtError> for AuthenticationError {
    fn from(error: JwtError) -> Self {
        match *error.kind() {
            ErrorKind::ExpiredSignature => Self::ExpiredToken,
            ErrorKind::ImmatureSignature => Self::ImmatureToken,
            _ => Self::InvalidToken,
        }
    }
}

impl ResponseError for AuthenticationError {
    fn status_code(&self) -> StatusCode {
        match self {
            AuthenticationError::ExpiredToken
            | AuthenticationError::ImmatureToken
            | AuthenticationError::InvalidToken => StatusCode::UNAUTHORIZED,
            AuthenticationError::InsufficientPermission
            | AuthenticationError::LockedUser
            | AuthenticationError::UnknownUser => StatusCode::FORBIDDEN,
            AuthenticationError::MissingHeader | AuthenticationError::WrongHeader => {
                StatusCode::BAD_REQUEST
            }
            AuthenticationError::APIError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        StatusResponse {
            message: format!("{}", self),
            code: self.status_code().as_u16(),
        }
        .into()
    }
}

#[derive(Debug, Clone)]
pub struct Config<'a> {
    enc_key: EncodingKey,
    dec_key: DecodingKey<'a>,
    validation: Validation,
}

impl<'a> Config<'a> {
    pub fn new<A>(secret: String, audience: A) -> Self
    where
        A: Into<Vec<String>>,
    {
        let mut validation = Validation {
            leeway: 10,
            ..Validation::default()
        };
        validation.set_audience(&audience.into());

        Self {
            enc_key: EncodingKey::from_secret(&secret.as_ref()),
            dec_key: DecodingKey::from_secret(&secret.as_ref()).into_static(),
            validation,
        }
    }

    pub fn from_env() -> Result<Self, Error> {
        let secret = match env::var("JWT_SECRET") {
            Ok(s) => s,
            Err(_) => return Err(Error::MissingEnvVar("JWT_SECRET".into())),
        };
        let audience = match env::var("JWT_AUDIENCE") {
            Ok(s) => s
                .split(',')
                .into_iter()
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>(),
            Err(_) => return Err(Error::MissingEnvVar("JWT_AUDIENCE".into())),
        };

        Ok(Self::new(secret, audience))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Claims {
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
        A: Into<Vec<String>>,
        S: Into<Vec<Scope>>,
    {
        Self {
            aud: aud.into(),
            exp: Utc::now() + Duration::minutes(Self::DEFAULT_EXP_MINUTES),
            iat: Utc::now(),
            sub: sub.into(),
            scope: scope.into(),
        }
    }

    pub fn set_expiration(&mut self, date: DateTime<Utc>) {
        self.exp = date;
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRequest {
    sub: String,
    scope: Vec<Scope>,
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    valid_for: Option<time::Duration>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    token: String,
    #[serde(with = "ts_seconds")]
    expires: DateTime<Utc>,
}

impl Responder for TokenResponse {
    fn respond_to(self, _req: &HttpRequest) -> HttpResponse {
        HttpResponse::Created().json(web::Json(self))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Authentication {
    scope: Option<Scope>,
}

impl Authentication {
    pub fn new() -> Self {
        Self { scope: None }
    }

    pub fn with_scope(scope: Scope) -> Self {
        Self { scope: Some(scope) }
    }

    pub async fn post_handler(
        req: HttpRequest,
        data: web::Json<TokenRequest>,
    ) -> actix_web::Result<TokenResponse> {
        let config = req.app_data::<Config>().unwrap();
        let client = req.app_data::<Mutex<Client>>().unwrap();

        let user = Self::get_user(&data.sub, client).await?;

        if user.locked {
            return Err(AuthenticationError::LockedUser.into());
        }

        let audience = config.validation.aud.to_owned().unwrap();

        let mut claims = Claims::new(Vec::from_iter(audience), &data.sub, data.scope.to_owned());

        if let Some(d) = data.valid_for {
            if let Ok(d) = Duration::from_std(d) {
                claims.set_expiration(claims.iat + d);
            }
        }

        let token = match encode(&Header::default(), &claims, &config.enc_key) {
            Ok(t) => t,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        Ok(TokenResponse {
            token,
            expires: claims.exp,
        })
    }

    pub async fn get_handler(req: HttpRequest, creds: Bearer) -> actix_web::Result<TokenResponse> {
        let config = req.app_data::<Config>().unwrap();

        let mut claims = Self::validate(&creds.token, config, true)?;

        let client = req.app_data::<Mutex<Client>>().unwrap();

        let user = Self::get_user(&claims.sub, client).await?;

        if user.locked {
            return Err(AuthenticationError::LockedUser.into());
        }

        claims.set_expiration(Utc::now() + Duration::minutes(Claims::DEFAULT_EXP_MINUTES));

        let token = match encode(&Header::default(), &claims, &config.enc_key) {
            Ok(t) => t,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        Ok(TokenResponse {
            token,
            expires: claims.exp,
        })
    }

    fn validate(
        token: &str,
        config: &Config,
        ignore_exp: bool,
    ) -> Result<Claims, AuthenticationError> {
        let validation = if ignore_exp {
            Validation {
                validate_exp: false,
                ..config.validation.clone()
            }
        } else {
            config.validation.clone()
        };

        let claims = match decode::<Claims>(token, &config.dec_key, &validation) {
            Ok(d) => d.claims,
            Err(e) => return Err(e.into()),
        };

        Ok(claims)
    }

    async fn get_user(user_id: &str, client: &Mutex<Client>) -> Result<User, AuthenticationError> {
        let mut c_client = client.lock().await;

        if !c_client.token_is_valid() {
            if let Err(e) = c_client.refresh_token().await {
                return Err(e.into());
            }
        }

        let user = match c_client.get_user_by_id(user_id).await {
            Ok(u) => u,
            Err(e) => match e {
                api::Error::ResourceNotFound => return Err(AuthenticationError::UnknownUser),
                _ => return Err(e.into()),
            },
        };

        Ok(user)
    }
}

impl<S, B> Transform<S, ServiceRequest> for Authentication
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Transform = AuthenticationMiddlware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthenticationMiddlware {
            scope: self.scope.clone(),
            service: Rc::new(RefCell::new(service)),
        }))
    }
}

pub struct AuthenticationMiddlware<S> {
    scope: Option<Scope>,
    service: Rc<RefCell<S>>,
}

impl<S, B> Service<ServiceRequest> for AuthenticationMiddlware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = S::Error;
    type Future = LocalBoxFuture<'static, Result<ServiceResponse<B>, actix_web::Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();

        let scope = self.scope.clone();

        async move {
            let config = req.app_data::<Config>().unwrap();

            let token = match Bearer::from_service_request(&req) {
                Ok(t) => t.token,
                Err(e) => return Err(e.into()),
            };

            let claims = match Authentication::validate(&token, &config, false) {
                Ok(c) => c,
                Err(e) => return Err(e.into()),
            };

            if let Some(scope) = scope {
                if !claims.scope.contains(&scope) {
                    return Err(AuthenticationError::InsufficientPermission.into());
                }
            }

            service.call(req).await
        }
        .boxed_local()
    }
}

#[derive(Debug, Deserialize)]
pub struct Bearer {
    pub token: String,
}

impl Bearer {
    fn from_service_request(req: &ServiceRequest) -> Result<Self, AuthenticationError> {
        Self::from_headers(req.headers())
    }

    fn from_headers(headers: &HeaderMap) -> Result<Self, AuthenticationError> {
        let value = match headers.get(header::AUTHORIZATION) {
            Some(h) => h.to_str().unwrap(),
            None => return Err(AuthenticationError::MissingHeader),
        };
        let token = if value.starts_with("Bearer ") {
            value.strip_prefix("Bearer ").unwrap().to_string()
        } else {
            return Err(AuthenticationError::WrongHeader);
        };

        Ok(Bearer { token })
    }
}

impl FromRequest for Bearer {
    type Error = AuthenticationError;
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        let b = match Self::from_headers(req.headers()) {
            Ok(b) => b,
            Err(e) => return ready(Err(e)),
        };

        ready(Ok(b))
    }
}
