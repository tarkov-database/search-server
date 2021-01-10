use crate::Error;

use std::env;

pub use tarkov_database_rs::client::{Client, ClientBuilder};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct ClientConfig;

impl ClientConfig {
    pub fn from_env() -> Result<ClientBuilder, Error> {
        let host = match env::var("API_HOST") {
            Ok(s) => s,
            Err(_) => return Err(Error::MissingEnvVar("API_HOST".into())),
        };
        let token = match env::var("API_TOKEN") {
            Ok(s) => s,
            Err(_) => return Err(Error::MissingEnvVar("API_TOKEN".into())),
        };

        let client_builder = ClientBuilder::default()
            .set_token(&token)
            .set_host(&host)
            .set_user_agent(USER_AGENT);

        let client_builder = if let Ok(ca) = env::var("API_CLIENT_CA") {
            client_builder.set_ca(ca)
        } else {
            client_builder
        };

        if let Ok(key) = env::var("API_CLIENT_KEY") {
            let cert = match env::var("API_CLIENT_CERT") {
                Ok(s) => s,
                Err(_) => return Err(Error::MissingEnvVar("API_CLIENT_KEY".into())),
            };
            Ok(client_builder.set_keypair(cert, key))
        } else {
            Ok(client_builder)
        }
    }
}
