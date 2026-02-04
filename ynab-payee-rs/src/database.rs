use dioxus::prelude::{debug, error, trace};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, Serializer};
use wasm_bindgen::JsValue;

use crate::{
    models::{Payee, ResponseData},
    settings::{
        DATABASE_NAME, DATABASE_VERSION, KNOWLEDGE_STORE_NAME, PAYEES_STORE_NAME,
        SERVER_KNOWLEDGE_KEY_PAYEES,
    },
};

pub async fn create_database() -> Result<Database, idb::Error> {
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
        knowledge_params.auto_increment(true);

        // knowledge_params.key_path(Some(KeyPath::new_single("name")));
        knowledge_params.key_path(None);
        let knowledge_store = database
            .create_object_store(KNOWLEDGE_STORE_NAME, knowledge_params)
            .map_err(|e| error!("unable to create server_knowledge store: {:#?}", e));
    });

    open_request.await
}

pub async fn replace_payees(
    database: &Database,
    data: &ResponseData,
) -> anyhow::Result<(Vec<JsValue>, JsValue)> {
    debug!("adding {} payees", &data.payees.len());
    let transaction = database
        .transaction(
            &[PAYEES_STORE_NAME, KNOWLEDGE_STORE_NAME],
            idb::TransactionMode::ReadWrite,
        )
        .map_err(|e| anyhow::anyhow!("unable to start transaction: {:#?}", e))?;

    let store = transaction
        .object_store(PAYEES_STORE_NAME)
        .map_err(|e| anyhow::anyhow!("unable to get object store: {:#?}", e))?;

    store
        .clear()
        .map_err(|e| anyhow::anyhow!("unable to clear the store: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to clear the store: {:#?}", e))?;

    let mut ids = Vec::new();
    for payee in data.payees.iter() {
        trace!("adding payee: {:#?}", &payee);
        let id = store
            .add(
                &payee
                    .serialize(&Serializer::json_compatible())
                    .map_err(|e| anyhow::anyhow!("unable to serialize payee: {:#?}", e))?,
                None,
            )
            .map_err(|e| anyhow::anyhow!("unable to store payee: {:#?}", e))?
            .await
            .map_err(|e| anyhow::anyhow!("unable to store payee: {:#?}", e))?;

        trace!("add payee: {:#?}", &payee);
        ids.push(id);
    }

    let kstore = transaction
        .object_store(KNOWLEDGE_STORE_NAME)
        .map_err(|e| anyhow::anyhow!("unable to get object store: {:#?}", e))?;

    kstore
        .clear()
        .map_err(|e| anyhow::anyhow!("unable to clear the store: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to clear the store: {:#?}", e))?;

    let knowledge = kstore
        .add(
            &data
                .server_knowledge
                .serialize(&Serializer::json_compatible())
                .map_err(|e| anyhow::anyhow!("unable to serialize server_knowledge: {:#?}", e))?,
            Some(&JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES)),
        )
        .map_err(|e| anyhow::anyhow!("unable to store server_knowledge: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to store server_knowledge: {:#?}", e))?;

    debug!("added server knowledge: {:#?}", &knowledge);

    transaction
        .commit()
        .map_err(|e| anyhow::anyhow!("unable to commit transaction: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to commit transaction: {:#?}", e))?;

    debug!("added {:#?} payees", &ids.len());

    Ok((ids, knowledge))
}

pub async fn get_payees(database: &Database) -> anyhow::Result<(Vec<Payee>, u64)> {
    let transaction = database
        .transaction(
            &[PAYEES_STORE_NAME, KNOWLEDGE_STORE_NAME],
            idb::TransactionMode::ReadOnly,
        )
        .map_err(|e| anyhow::anyhow!("unable to start tranaction: {:#?}", e))?;

    let store = transaction
        .object_store(PAYEES_STORE_NAME)
        .map_err(|e| anyhow::anyhow!("unable to get object store: {:#?}", e))?;

    let stored_payees = store
        .get_all(None, None)
        .map_err(|e| anyhow::anyhow!("unable to get stored payees: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to get stored payees: {:#?}", e))?;

    let stored_payees = stored_payees
        .into_iter()
        .map(|p| {
            serde_wasm_bindgen::from_value(p.clone())
                .map_err(|e| anyhow::anyhow!("unable to map payee: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<Payee>>>()
        .map_err(|e| anyhow::anyhow!("unable to get stored payees: {:#?}", e))?;

    let kstore = transaction
        .object_store(KNOWLEDGE_STORE_NAME)
        .map_err(|e| anyhow::anyhow!("unable to get object store: {:#?}", e))?;

    let knowledge = kstore
        .get(JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES))
        .map_err(|e| anyhow::anyhow!("unable to get stored payees: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to await stored payees: {:#?}", e))?
        .ok_or_else(|| anyhow::anyhow!("no server knowledge found in indexdb"))?;

    let knowledge = from_value::<u64>(knowledge).map_err(|e| {
        anyhow::anyhow!(
            "unable to deserialize server knowledge from indexdb: {:#?}",
            e
        )
    })?;

    transaction
        .await
        .map_err(|e| anyhow::anyhow!("unable to await transaction: {:#?}", e))?;

    Ok((stored_payees, knowledge))
}
