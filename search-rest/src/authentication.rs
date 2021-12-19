use crate::{error, model::Status};

use hyper::StatusCode;
use jsonwebtoken::{
    errors::{Error as JwtError, ErrorKind},
    Algorithm, DecodingKey, EncodingKey, Validation,
};
use serde::{de::DeserializeOwned, Serialize};
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Missing authorization header")]
    MissingHeader,
    #[error("header error: {0}")]
    InvalidHeader(String),
    #[error("Insufficient permission")]
    InsufficientPermission,
    #[error("User is blocked")]
    LockedUser,
    #[error("User doesn't exist")]
    UnknownUser,
    #[error("token error: {0}")]
    Token(#[from] TokenError),
}

impl error::ErrorResponse for AuthenticationError {
    type Response = Status;

    fn status_code(&self) -> StatusCode {
        match self {
            AuthenticationError::MissingHeader
            | AuthenticationError::InvalidHeader(_)
            | AuthenticationError::Token(_) => StatusCode::UNAUTHORIZED,
            AuthenticationError::LockedUser
            | AuthenticationError::InsufficientPermission
            | AuthenticationError::UnknownUser => StatusCode::FORBIDDEN,
        }
    }

    fn error_response(&self) -> Self::Response {
        Status::new(self.status_code(), self.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token is expired")]
    Expired,
    #[error("token is not yet valid")]
    Immature,
    #[error("token is invalid")]
    Invalid,
    #[error("Token could not be encoded: {0}")]
    EncodingFailed(JwtError),
}

impl From<JwtError> for TokenError {
    fn from(error: JwtError) -> Self {
        match *error.kind() {
            ErrorKind::ExpiredSignature => Self::Expired,
            ErrorKind::ImmatureSignature => Self::Immature,
            ErrorKind::InvalidToken => Self::Invalid,
            _ => {
                error!(error = ?error, "JWT error");
                Self::Invalid
            }
        }
    }
}

impl error::ErrorResponse for TokenError {
    type Response = Status;

    fn status_code(&self) -> StatusCode {
        match self {
            TokenError::Expired => StatusCode::UNAUTHORIZED,
            TokenError::Immature => StatusCode::UNAUTHORIZED,
            TokenError::Invalid => StatusCode::UNAUTHORIZED,
            TokenError::EncodingFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> Self::Response {
        Status::new(self.status_code(), self.to_string())
    }
}

pub trait TokenClaims
where
    Self: Serialize + DeserializeOwned + Sized,
{
    fn decode(token: &str, config: &TokenConfig, validate_exp: bool) -> Result<Self, TokenError> {
        let validation = if !validate_exp {
            Validation {
                validate_exp,
                ..config.validation.clone()
            }
        } else {
            config.validation.clone()
        };

        let data = jsonwebtoken::decode::<Self>(token, &config.dec_key, &validation)?;

        Ok(data.claims)
    }

    fn encode(&self, config: &TokenConfig) -> Result<String, TokenError> {
        let header = jsonwebtoken::Header::new(config.alg);
        let token = jsonwebtoken::encode(&header, self, &config.enc_key).map_err(|e| {
            error!(error = ?e, "Error while encoding token");
            TokenError::EncodingFailed(e)
        })?;

        Ok(token)
    }
}

#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub alg: Algorithm,
    pub enc_key: EncodingKey,
    pub dec_key: DecodingKey<'static>,
    pub validation: Validation,
}

impl TokenConfig {
    const LEEWAY: u64 = 10;

    pub fn from_secret<S, A, T>(secret: S, audience: A) -> Self
    where
        S: AsRef<[u8]>,
        A: AsRef<[T]>,
        T: ToString,
    {
        let mut validation = Validation {
            leeway: Self::LEEWAY,
            ..Validation::default()
        };
        validation.set_audience(audience.as_ref());

        Self {
            alg: Algorithm::HS256,
            enc_key: EncodingKey::from_secret(secret.as_ref()),
            dec_key: DecodingKey::from_secret(secret.as_ref()).into_static(),
            validation,
        }
    }
}
