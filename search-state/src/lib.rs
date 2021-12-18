use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::{DateTime, TimeZone, Utc};
use tarkov_database_rs::{client::Client, model::item::Item};
use thiserror::Error;
use tokio::sync::{broadcast::Receiver, RwLock};
use tracing::{error, info};

use search_index::Index;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Index error: {0}")]
    IndexError(#[from] search_index::Error),
    #[error("API error: {0}")]
    ApiError(#[from] tarkov_database_rs::Error),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct IndexState {
    index: Index,
    modified: Arc<RwLock<DateTime<Utc>>>,
}

impl IndexState {
    pub fn new(index: Index) -> Self {
        Self {
            index,
            modified: Arc::new(RwLock::new(Utc.timestamp(0, 0))),
        }
    }

    pub fn get_index(&self) -> Index {
        self.index.clone()
    }

    pub async fn get_modified(&self) -> DateTime<Utc> {
        *self.modified.read().await
    }

    pub async fn update_items(&self, items: Vec<Item>) -> Result<()> {
        let mut c_modified = self.modified.write().await;

        self.index.write_index(items)?;

        *c_modified = Utc::now();

        Ok(())
    }
}

pub struct IndexStateHandler {
    state: IndexState,
    client: Client,
    status: Arc<HandlerStatus>,
    interval: Duration,
}

impl IndexStateHandler {
    pub fn new(index: IndexState, client: Client, interval: Duration) -> Self {
        Self {
            state: index,
            client,
            interval,
            status: Arc::new(HandlerStatus::default()),
        }
    }

    pub fn status_ref(&self) -> Arc<HandlerStatus> {
        self.status.clone()
    }

    async fn update_state(&mut self) {
        if !self.client.token_is_valid().await {
            if let Err(e) = self.client.refresh_token().await {
                error!(
                    "Couldn't update index: error while refreshing API token: {}",
                    e
                );
                self.status.set_client_error(true);
                return;
            }
        }

        let stats = match self.client.get_item_index().await {
            Ok(i) => i,
            Err(e) => {
                error!(
                    "Couldn't update index: error while getting item index: {}",
                    e
                );
                self.status.set_client_error(true);
                return;
            }
        };

        if self.state.get_modified().await.lt(&stats.modified) {
            info!("Item index are out of date. Perform update...");

            let items = match self.client.get_items_all().await {
                Ok(d) => d,
                Err(e) => {
                    error!(
                        "Couldn't update index: error while getting items from API: {}",
                        e
                    );
                    self.status.set_client_error(true);
                    return;
                }
            };

            if let Err(e) = self.state.update_items(items).await {
                error!(
                    "Couldn't update index: error while writing item index: {}",
                    e
                );
                self.status.set_index_error(true);
                return;
            }

            if let Err(e) = self.state.index.check_health() {
                error!("Error while checking index health: {}", e);
                self.status.set_index_error(true);
                return;
            }
        }

        self.status.set_client_error(false);
        self.status.set_index_error(false);
    }

    pub async fn run(mut self, mut shutdown: Receiver<()>) -> Result<()> {
        let mut interval = tokio::time::interval(self.interval);

        tracing::debug!(
            "watching for changes every {}s",
            self.interval.as_secs_f64()
        );

        loop {
            tokio::select! {
                biased;
                _ = shutdown.recv() => break,
                _ = interval.tick() => {},
            };

            self.update_state().await;
        }

        tracing::debug!("shutting down...");

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct HandlerStatus {
    index_error: AtomicBool,
    client_error: AtomicBool,
}

impl HandlerStatus {
    pub fn set_index_error(&self, val: bool) {
        tracing::debug!(value = ?val, "index error set");
        self.index_error.store(val, Ordering::SeqCst);
    }

    pub fn set_client_error(&self, val: bool) {
        tracing::debug!(value = ?val, "client error set");
        self.client_error.store(val, Ordering::SeqCst);
    }

    pub fn is_index_error(&self) -> bool {
        self.index_error.load(Ordering::SeqCst)
    }

    pub fn is_client_error(&self) -> bool {
        self.client_error.load(Ordering::SeqCst)
    }
}
