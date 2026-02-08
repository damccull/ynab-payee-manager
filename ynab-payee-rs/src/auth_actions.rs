use std::sync::Arc;

use dioxus::{
    core::spawn,
    signals::{ReadableExt, Signal, WritableExt},
};
use idb::Database;

use crate::database::{get_ynab_token, store_ynab_token};
pub enum AuthActionStatus {
    Idle,
    Loading,
    Saving,
    Error(anyhow::Error),
}

#[derive(Clone)]
pub struct AuthActions {
    db: Arc<Database>,
    pub token: Signal<Option<String>>,
    pub status: Signal<AuthActionStatus>,
}

impl AuthActions {
    pub fn new(db: Arc<Database>) -> Self {
        let aa = Self {
            db,
            status: Signal::new(AuthActionStatus::Idle),
            token: Signal::new(None),
        };
        aa.get_token();
        aa
    }

    pub fn store_token(&self, token: String) {
        let db = self.db.clone();
        let mut status = self.status;
        let mut thetoken = self.token;
        spawn(async move {
            status.set(AuthActionStatus::Saving);

            match store_ynab_token(&db, &token).await {
                Ok(val) => thetoken.set(Some(val)),
                Err(e) => {
                    status.set(AuthActionStatus::Error(anyhow::anyhow!(
                        "unable to store token: {:#?}",
                        e
                    )));
                }
            }

            status.set(AuthActionStatus::Idle);
        });
    }

    // Not public because it should be called by the new() fn to hydrate on creation
    fn get_token(&self) {
        let db = self.db.clone();
        let mut status = self.status;
        let mut token = self.token;
        spawn(async move {
            status.set(AuthActionStatus::Loading);

            match get_ynab_token(&db).await {
                Ok(val) => token.set(val),
                Err(e) => {
                    status.set(AuthActionStatus::Error(anyhow::anyhow!(
                        "unable to get token from indexdb: {:#?}",
                        e
                    )));
                }
            }

            status.set(AuthActionStatus::Idle);
        });
    }
}
