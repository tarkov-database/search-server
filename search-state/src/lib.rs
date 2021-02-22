use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use actix::{fut::wrap_future, Actor, AsyncContext, Context};
use chrono::{DateTime, TimeZone, Utc};
use log::{error, info};
use tarkov_database_rs::{client::Client, model::item::Item};
use thiserror::Error;
use tokio::sync::Mutex;

use search_index::Index;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Index error: {}", _0)]
    IndexError(#[from] search_index::Error),
    #[error("API error: {}", _0)]
    APIError(#[from] tarkov_database_rs::Error),
}

type Result<T> = std::result::Result<T, Error>;

pub struct IndexState {
    pub index: Index,
    modified: RwLock<DateTime<Utc>>,
}

impl IndexState {
    pub fn new(index: Index) -> Self {
        Self {
            index,
            modified: RwLock::new(Utc.timestamp(0, 0)),
        }
    }

    pub fn get_modified(&self) -> DateTime<Utc> {
        self.modified.read().unwrap().to_owned()
    }

    pub fn update_items(&self, items: Vec<Item>) -> Result<()> {
        let mut c_modified = self.modified.write().unwrap();

        self.index.write_index(items)?;

        *c_modified = Utc::now();

        Ok(())
    }
}

pub struct IndexStateHandler {
    state: Arc<IndexState>,
    client: Arc<Mutex<Client>>,
    status: Arc<HandlerStatus>,
    interval: Duration,
}

impl IndexStateHandler {
    pub fn new(index: Arc<IndexState>, client: Client, interval: Duration) -> Self {
        Self {
            state: index,
            client: Arc::new(Mutex::new(client)),
            interval,
            status: Arc::new(HandlerStatus::default()),
        }
    }

    pub fn status_ref(&self) -> Arc<HandlerStatus> {
        self.status.clone()
    }

    fn update_state(&mut self, ctx: &mut Context<Self>) {
        let client = self.client.clone();
        let state = self.state.clone();
        let status = self.status.clone();

        ctx.spawn(wrap_future(async move {
            let mut client = client.lock().await;

            if !client.token_is_valid() {
                if let Err(e) = client.refresh_token().await {
                    error!(
                        "Couldn't update index: error while refreshing API token: {}",
                        e
                    );
                    status.set_client_error(true);
                    return;
                }
            }

            let stats = match client.get_item_index().await {
                Ok(i) => i,
                Err(e) => {
                    error!(
                        "Couldn't update index: error while getting item index: {}",
                        e
                    );
                    status.set_client_error(true);
                    return;
                }
            };

            if state.get_modified().lt(&stats.modified) {
                info!("Item index are out of date. Perform update...");

                let items = match client.get_items_all().await {
                    Ok(d) => d,
                    Err(e) => {
                        error!(
                            "Couldn't update index: error while getting items from API: {}",
                            e
                        );
                        status.set_client_error(true);
                        return;
                    }
                };

                if let Err(e) = state.update_items(items) {
                    error!(
                        "Couldn't update index: error while writing item index: {}",
                        e
                    );
                    status.set_index_error(true);
                    return;
                }

                if let Err(e) = state.index.check_health() {
                    error!("Error while checking index health: {}", e);
                    status.set_index_error(true);
                    return;
                }
            }

            status.set_client_error(false);
            status.set_index_error(false);
        }));
    }
}

impl Actor for IndexStateHandler {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(self.interval, Self::update_state);
    }
}

#[derive(Debug, Default)]
pub struct HandlerStatus {
    index_error: AtomicBool,
    client_error: AtomicBool,
}

impl HandlerStatus {
    pub fn set_index_error(&self, val: bool) {
        self.index_error.store(val, Ordering::SeqCst);
    }

    pub fn set_client_error(&self, val: bool) {
        self.client_error.store(val, Ordering::SeqCst);
    }

    pub fn is_index_error(&self) -> bool {
        self.index_error.load(Ordering::SeqCst)
    }

    pub fn is_client_error(&self) -> bool {
        self.client_error.load(Ordering::SeqCst)
    }
}
