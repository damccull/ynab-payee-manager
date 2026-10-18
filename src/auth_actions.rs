use std::sync::Arc;

use dioxus::{
    core::spawn,
    prelude::{debug, error},
    signals::{Signal, WritableExt},
};
use idb::Database;

use crate::{
    database::{get_ynab_token, store_ynab_token},
    YnabToken,
};

#[derive(Debug)]
pub enum AuthActionStatus {
    Idle,
    Loading,
    Saving,
    Error(anyhow::Error),
}

#[derive(Clone, Debug)]
pub struct AuthActions {
    db: Arc<Database>,
    pub token: Signal<Option<YnabToken>>,
    pub status: Signal<AuthActionStatus>,
}

impl AuthActions {
    /// Creates a new AuthActions and hydrates the token ([`YnabToken`]) from indexdb if it exists,
    /// or stores the initial token if provided.
    pub fn new(db: Arc<Database>, initial_token: Option<YnabToken>) -> Self {
        let aa = Self {
            db,
            status: Signal::new(AuthActionStatus::Idle),
            token: Signal::new(initial_token.clone()),
        };
        if let Some(tok) = initial_token {
            aa.store_token(tok);
        } else {
            aa.get_token();
        }
        aa
    }

    /// Stores a token in the indexdb and hydrates this [`AuthActions`]'s [`YnabToken`]
    pub fn store_token(&self, token: YnabToken) {
        debug!("storing token: {:#?}", &token);
        let db = self.db.clone();
        let mut status = self.status;
        let mut thetoken = self.token;
        spawn(async move {
            status.set(AuthActionStatus::Saving);

            match store_ynab_token(&db, token).await {
                Ok(val) => thetoken.set(Some(val)),
                Err(e) => {
                    error!("Unable to save token to database: {}", &e);
                    status.set(AuthActionStatus::Error(anyhow::anyhow!(
                        "unable to store token: {:#?}",
                        e
                    )));
                    thetoken.set(None);
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
                Ok(val) => {
                    debug!("token retrieved from indexdb: {:#?}", &val);
                    token.set(val);
                }
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
