use std::{sync::Arc, time::Duration};

use actix::{fut::wrap_future, Actor, AsyncContext, Context};
use chrono::{DateTime, TimeZone, Utc};
use log::{error, info};
use search_index::ItemIndex;
use tokio::sync::Mutex;

use tarkov_database_rs::client::Client;

pub struct IndexStateHandler {
    pub client: Arc<Mutex<Client>>,
    pub interval: Duration,
    pub item_index: Option<Arc<IndexState<ItemIndex>>>,
}

pub struct IndexState<T> {
    pub index: T,
    modified: Mutex<DateTime<Utc>>,
}

impl<T> IndexState<T> {
    pub fn new(index: T) -> Self {
        Self {
            index,
            modified: Mutex::new(Utc.timestamp(0, 0)),
        }
    }
}

impl IndexStateHandler {
    pub fn new(client: Client, interval: Duration) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            interval,
            item_index: None,
        }
    }

    fn update_indexes(&mut self, ctx: &mut Context<Self>) {
        let client = self.client.clone();
        let item_index = self.item_index.clone();

        ctx.spawn(wrap_future(async move {
            let mut c_client = client.lock().await;

            if !c_client.token_is_valid() {
                if let Err(e) = c_client.refresh_token().await {
                    error!(
                        "Couldn't update indexes: error while refreshing API token: {}",
                        e
                    );
                    return;
                }
            }

            if let Some(state) = item_index {
                let stats = match c_client.get_item_index().await {
                    Ok(i) => i,
                    Err(e) => {
                        error!(
                            "Couldn't update indexes: error while getting item index: {}",
                            e
                        );
                        return;
                    }
                };

                let mut c_modified = state.modified.lock().await;

                if c_modified.lt(&stats.modified) {
                    info!("Item index are out of date. Perform update...");

                    let items = match c_client.get_items_all().await {
                        Ok(d) => d,
                        Err(e) => {
                            error!(
                                "Couldn't update indexes: error while getting items from API: {}",
                                e
                            );
                            return;
                        }
                    };

                    if let Err(e) = state.index.write_index(items) {
                        error!(
                            "Couldn't update indexes: error while writing item index: {}",
                            e
                        );
                        return;
                    }

                    *c_modified = stats.modified;
                }
            }
        }));
    }

    pub fn set_item_index(&mut self, index: Arc<IndexState<ItemIndex>>) {
        self.item_index = Some(index);
    }
}

impl Actor for IndexStateHandler {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.update_indexes(ctx);
        ctx.run_interval(self.interval, Self::update_indexes);
    }
}
