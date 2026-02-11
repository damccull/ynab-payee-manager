use dioxus::prelude::{debug, error, trace};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, Serializer};
use wasm_bindgen::JsValue;

use crate::{
    models::{Payee, ResponseData},
    settings::{
        DATABASE_NAME, DATABASE_VERSION, KNOWLEDGE_STORE_NAME, PAYEES_STORE_NAME,
        SERVER_KNOWLEDGE_KEY_PAYEES, SETTINGS_KEY_TOKEN, SETTINGS_STORE_NAME,
    },
    YnabToken,
};

pub async fn create_database() -> Result<Database, DatabaseError> {
    let factory = Factory::new()?;
    let mut open_request = factory
        .open(DATABASE_NAME, Some(DATABASE_VERSION))
        .expect("unable to open database");

    // Add an upgrade handler for the database
    open_request.on_upgrade_needed(|event| {
        // Get database instance from event
        let database = event.database().expect("unable to get database from event");

        database.delete_object_store(PAYEES_STORE_NAME);
        database.delete_object_store(KNOWLEDGE_STORE_NAME);

        // Prepare object store parameters
        let mut payees_params = ObjectStoreParams::new();
        payees_params.auto_increment(true);
        payees_params.key_path(Some(KeyPath::new_single("id")));

        // Create 'payees' object store
        let payees_store = database
            .create_object_store(PAYEES_STORE_NAME, payees_params)
            .map_err(|e| error!("unable to create payees store: {:#?}", e))
            .expect("unable to get object store for payees");

        // Prepare index parameters
        let mut payees_index_parameters = IndexParams::new();
        payees_index_parameters.unique(false);

        // Create an index for 'name' on the payees_store
        payees_store
            .create_index(
                "name",
                KeyPath::new_single("name"),
                Some(payees_index_parameters),
            )
            .map_err(|e| error!("unable to create index: {:#?}", e));

        // Prepare object store parameters
        let mut knowledge_params = ObjectStoreParams::new();
        knowledge_params.auto_increment(false);

        // knowledge_params.key_path(Some(KeyPath::new_single("name")));
        knowledge_params.key_path(None);
        let knowledge_store = database
            .create_object_store(KNOWLEDGE_STORE_NAME, knowledge_params)
            .map_err(|e| error!("unable to create server_knowledge store: {:#?}", e));

        let mut settings_params = ObjectStoreParams::new();
        settings_params.auto_increment(false);

        let settings_store = database
            .create_object_store(SETTINGS_STORE_NAME, settings_params)
            .map_err(|e| error!("unable to create settings store: {:#?}", e))
            .expect("unable to get object store for settings");
    });

    Ok(open_request.await?)
}

pub async fn store_ynab_token(
    database: &Database,
    token: YnabToken,
) -> Result<YnabToken, DatabaseError> {
    let transaction =
        database.transaction(&[SETTINGS_STORE_NAME], idb::TransactionMode::ReadWrite)?;

    let store = transaction.object_store(SETTINGS_STORE_NAME)?;

    let id = store
        .put(
            &token
                .serialize(&Serializer::json_compatible())
                .map_err(|e| anyhow::anyhow!("unable to serialize setting: {:#?}", e))?,
            Some(&JsValue::from_str(SETTINGS_KEY_TOKEN)),
        )?
        .await?;
    Ok(token.to_owned())
}

pub async fn get_ynab_token(database: &Database) -> Result<Option<YnabToken>, DatabaseError> {
    let transaction =
        database.transaction(&[SETTINGS_STORE_NAME], idb::TransactionMode::ReadOnly)?;

    let store = transaction.object_store(SETTINGS_STORE_NAME)?;

    let token = store.get(JsValue::from_str(SETTINGS_KEY_TOKEN))?.await?;

    transaction.await?;

    // Deserialize the JsValue

    let token =
        if let Some(token) = token {
            Some(from_value::<YnabToken>(token).map_err(|e| {
                anyhow::anyhow!("unable to deserialize token from indexdb: {:#?}", e)
            })?)
        } else {
            None
        };
    // let token = from_value::<Option<String>>(token).map_err(|e| {
    //     anyhow::anyhow!(
    //         "unable to deserialize server knowledge from indexdb: {:#?}",
    //         e
    //     )
    // })?;

    Ok(token)
}

pub async fn replace_payees(
    database: &Database,
    data: &ResponseData,
) -> Result<(Vec<JsValue>, JsValue), DatabaseError> {
    debug!("adding {} payees", &data.payees.len());
    let transaction = database.transaction(
        &[PAYEES_STORE_NAME, KNOWLEDGE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;

    let store = transaction.object_store(PAYEES_STORE_NAME)?;

    store.clear()?.await?;

    let mut ids = Vec::new();
    for payee in data.payees.iter() {
        trace!("adding payee: {:#?}", &payee);
        let id = store
            .add(
                &payee
                    .serialize(&Serializer::json_compatible())
                    .map_err(|e| anyhow::anyhow!("unable to serialize payee: {:#?}", e))?,
                None,
            )?
            .await?;

        trace!("add payee: {:#?}", &payee);
        ids.push(id);
    }

    let kstore = transaction.object_store(KNOWLEDGE_STORE_NAME)?;

    kstore.clear()?.await?;

    let knowledge = kstore
        .add(
            &data
                .server_knowledge
                .serialize(&Serializer::json_compatible())
                .map_err(|e| anyhow::anyhow!("unable to serialize server_knowledge: {:#?}", e))?,
            Some(&JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES)),
        )?
        .await?;

    debug!("added server knowledge: {:#?}", &knowledge);

    transaction.commit()?.await?;

    debug!("added {:#?} payees", &ids.len());

    Ok((ids, knowledge))
}

pub async fn get_payees(database: &Database) -> Result<(Vec<Payee>, u64), DatabaseError> {
    let transaction = database.transaction(
        &[PAYEES_STORE_NAME, KNOWLEDGE_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;

    let store = transaction.object_store(PAYEES_STORE_NAME)?;

    let stored_payees = store.get_all(None, None)?.await?;

    let stored_payees = stored_payees
        .into_iter()
        .map(|p| {
            serde_wasm_bindgen::from_value(p.clone())
                .map_err(|e| anyhow::anyhow!("unable to map payee: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<Payee>>>()?;

    let kstore = transaction.object_store(KNOWLEDGE_STORE_NAME)?;

    let knowledge = kstore
        .get(JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES))?
        .await?
        .ok_or_else(|| anyhow::anyhow!("no server knowledge found in indexdb"))?;

    let knowledge = from_value::<u64>(knowledge).map_err(|e| {
        anyhow::anyhow!(
            "unable to deserialize server knowledge from indexdb: {:#?}",
            e
        )
    })?;

    transaction.await?;

    Ok((stored_payees, knowledge))
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(transparent)]
    IndexDbError(#[from] idb::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}
