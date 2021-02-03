use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use actix::{fut::wrap_future, Actor, AsyncContext, Context};
use chrono::{DateTime, TimeZone, Utc};
use log::{error, info};
use search_index::{Error as IndexError, Index};
use tokio::sync::Mutex;

use tarkov_database_rs::{client::Client, model::item::Item};

pub struct IndexStateHandler {
    pub index: Arc<IndexState>,
    pub client: Arc<Mutex<Client>>,
    pub interval: Duration,
}

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

    pub fn update_items(&self, items: Vec<Item>) -> Result<(), IndexError> {
        let mut c_modified = self.modified.write().unwrap();

        self.index.write_index(items)?;

        *c_modified = Utc::now();

        Ok(())
    }

    pub fn get_modified(&self) -> DateTime<Utc> {
        self.modified.read().unwrap().to_owned()
    }
}

impl IndexStateHandler {
    pub fn new(index: Arc<IndexState>, client: Client, interval: Duration) -> Self {
        Self {
            index,
            client: Arc::new(Mutex::new(client)),
            interval,
        }
    }

    fn update_index(&mut self, ctx: &mut Context<Self>) {
        let client = self.client.clone();
        let index = self.index.clone();

        ctx.spawn(wrap_future(async move {
            let mut c_client = client.lock().await;

            if !c_client.token_is_valid() {
                if let Err(e) = c_client.refresh_token().await {
                    error!(
                        "Couldn't update index: error while refreshing API token: {}",
                        e
                    );
                    return;
                }
            }

            let stats = match c_client.get_item_index().await {
                Ok(i) => i,
                Err(e) => {
                    error!(
                        "Couldn't update index: error while getting item index: {}",
                        e
                    );
                    return;
                }
            };

            if index.get_modified().lt(&stats.modified) {
                info!("Item index are out of date. Perform update...");

                let items = match c_client.get_items_all().await {
                    Ok(d) => d,
                    Err(e) => {
                        error!(
                            "Couldn't update index: error while getting items from API: {}",
                            e
                        );
                        return;
                    }
                };

                if let Err(e) = index.update_items(items) {
                    error!(
                        "Couldn't update index: error while writing item index: {}",
                        e
                    );
                }
            }
        }));
    }
}

impl Actor for IndexStateHandler {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(self.interval, Self::update_index);
    }
}
