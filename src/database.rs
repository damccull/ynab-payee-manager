use chrono::Utc;
use dioxus::prelude::{debug, error, trace};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use serde::Serialize;
use serde_wasm_bindgen::{from_value, Serializer};
use wasm_bindgen::JsValue;

use crate::{
    models::{ActionStatus, Payee, PayeeAction, PayeeReviewItem, ResponseData},
    settings::{
        ACTION_QUEUE_STORE_NAME, DATABASE_NAME, DATABASE_VERSION, KNOWLEDGE_STORE_NAME,
        PAYEES_STORE_NAME, PAYEE_REVIEW_QUEUE_STORE_NAME, SERVER_KNOWLEDGE_KEY_PAYEES,
        SETTINGS_KEY_TOKEN, SETTINGS_STORE_NAME,
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
        let database = event.database().expect("unable to get database from event");
        let existing_stores = database.store_names();

        // Ensure 'payees' object store exists
        if !existing_stores.contains(&PAYEES_STORE_NAME.to_string()) {
            let mut payees_params = ObjectStoreParams::new();
            payees_params.auto_increment(false);
            payees_params.key_path(Some(KeyPath::new_single("id")));

            let payees_store = database
                .create_object_store(PAYEES_STORE_NAME, payees_params)
                .map_err(|e| error!("unable to create payees store: {:#?}", e))
                .expect("unable to get object store for payees");

            let mut payees_index_parameters = IndexParams::new();
            payees_index_parameters.unique(false);

            let _ = payees_store
                .create_index(
                    "name",
                    KeyPath::new_single("name"),
                    Some(payees_index_parameters),
                )
                .map_err(|e| error!("unable to create index: {:#?}", e));
        }

        // Ensure 'server_knowledge' object store exists
        if !existing_stores.contains(&KNOWLEDGE_STORE_NAME.to_string()) {
            let mut knowledge_params = ObjectStoreParams::new();
            knowledge_params.auto_increment(false);
            knowledge_params.key_path(None);
            let _knowledge_store = database
                .create_object_store(KNOWLEDGE_STORE_NAME, knowledge_params)
                .map_err(|e| error!("unable to create server_knowledge store: {:#?}", e));
        }

        // Ensure 'settings' object store exists
        if !existing_stores.contains(&SETTINGS_STORE_NAME.to_string()) {
            let mut settings_params = ObjectStoreParams::new();
            settings_params.auto_increment(false);

            let _settings_store = database
                .create_object_store(SETTINGS_STORE_NAME, settings_params)
                .map_err(|e| error!("unable to create settings store: {:#?}", e))
                .expect("unable to get object store for settings");
        }

        // Ensure 'payee_review_queue' object store exists
        if !existing_stores.contains(&PAYEE_REVIEW_QUEUE_STORE_NAME.to_string()) {
            let mut review_params = ObjectStoreParams::new();
            review_params.auto_increment(false);
            review_params.key_path(Some(KeyPath::new_single("id")));

            let review_store = database
                .create_object_store(PAYEE_REVIEW_QUEUE_STORE_NAME, review_params)
                .map_err(|e| error!("unable to create payee review queue store: {:#?}", e))
                .expect("unable to create payee review queue store");

            let mut review_index = IndexParams::new();
            review_index.unique(false);
            let _ = review_store
                .create_index(
                    "created_at",
                    KeyPath::new_single("created_at"),
                    Some(review_index),
                )
                .map_err(|e| error!("unable to create created_at index: {:#?}", e));
        }

        // Ensure 'action_queue' object store exists
        if !existing_stores.contains(&ACTION_QUEUE_STORE_NAME.to_string()) {
            let mut action_params = ObjectStoreParams::new();
            action_params.auto_increment(false);
            action_params.key_path(Some(KeyPath::new_single("id")));

            let action_store = database
                .create_object_store(ACTION_QUEUE_STORE_NAME, action_params)
                .map_err(|e| error!("unable to create action queue store: {:#?}", e))
                .expect("unable to create action queue store");

            let mut seq_index = IndexParams::new();
            seq_index.unique(false);
            let _ = action_store
                .create_index(
                    "sequence_number",
                    KeyPath::new_single("sequence_number"),
                    Some(seq_index),
                )
                .map_err(|e| error!("unable to create sequence_number index: {:#?}", e));

            let mut created_index = IndexParams::new();
            created_index.unique(false);
            let _ = action_store
                .create_index(
                    "created_at",
                    KeyPath::new_single("created_at"),
                    Some(created_index),
                )
                .map_err(|e| error!("unable to create created_at index on action queue: {:#?}", e));
        }
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

    let _id = store
        .put(
            &token
                .serialize(&Serializer::json_compatible())
                .map_err(|e| anyhow::anyhow!("unable to serialize setting: {:#?}", e))?,
            Some(&JsValue::from_str(SETTINGS_KEY_TOKEN)),
        )?
        .await?;
    transaction.commit()?.await?;
    Ok(token.to_owned())
}

pub async fn get_ynab_token(database: &Database) -> Result<Option<YnabToken>, DatabaseError> {
    let transaction =
        database.transaction(&[SETTINGS_STORE_NAME], idb::TransactionMode::ReadOnly)?;

    let store = transaction.object_store(SETTINGS_STORE_NAME)?;

    let token = store.get(JsValue::from_str(SETTINGS_KEY_TOKEN))?.await?;

    transaction.await?;

    let token = if let Some(token) = token {
        Some(from_value::<YnabToken>(token).map_err(|e| {
            anyhow::anyhow!("unable to deserialize token from indexdb: {:#?}", e)
        })?)
    } else {
        None
    };

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

// ----------------------------------------------------------------------------
// Payee Review Queue Database Operations
// ----------------------------------------------------------------------------

pub async fn save_review_item(
    database: &Database,
    item: &PayeeReviewItem,
) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[PAYEE_REVIEW_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(PAYEE_REVIEW_QUEUE_STORE_NAME)?;

    let js_val = item
        .serialize(&Serializer::json_compatible())
        .map_err(|e| anyhow::anyhow!("unable to serialize review item: {:#?}", e))?;

    store.put(&js_val, None)?.await?;
    transaction.commit()?.await?;
    Ok(())
}

pub async fn get_review_items(database: &Database) -> Result<Vec<PayeeReviewItem>, DatabaseError> {
    let transaction = database.transaction(
        &[PAYEE_REVIEW_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;
    let store = transaction.object_store(PAYEE_REVIEW_QUEUE_STORE_NAME)?;

    let stored = store.get_all(None, None)?.await?;
    let mut items = stored
        .into_iter()
        .map(|val| {
            serde_wasm_bindgen::from_value(val)
                .map_err(|e| anyhow::anyhow!("unable to map review item: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<PayeeReviewItem>>>()?;

    transaction.await?;

    // Sort newest first
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(items)
}

pub async fn delete_review_item(database: &Database, id: &str) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[PAYEE_REVIEW_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(PAYEE_REVIEW_QUEUE_STORE_NAME)?;
    store.delete(JsValue::from_str(id))?.await?;
    transaction.commit()?.await?;
    Ok(())
}

pub async fn clear_review_items(database: &Database) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[PAYEE_REVIEW_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(PAYEE_REVIEW_QUEUE_STORE_NAME)?;
    store.clear()?.await?;
    transaction.commit()?.await?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Action Queue Database Operations
// ----------------------------------------------------------------------------

pub async fn enqueue_action(
    database: &Database,
    action: &PayeeAction,
) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[ACTION_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(ACTION_QUEUE_STORE_NAME)?;

    let js_val = action
        .serialize(&Serializer::json_compatible())
        .map_err(|e| anyhow::anyhow!("unable to serialize action: {:#?}", e))?;

    store.put(&js_val, None)?.await?;
    transaction.commit()?.await?;
    Ok(())
}

pub async fn get_all_actions(database: &Database) -> Result<Vec<PayeeAction>, DatabaseError> {
    let transaction = database.transaction(
        &[ACTION_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;
    let store = transaction.object_store(ACTION_QUEUE_STORE_NAME)?;

    let stored = store.get_all(None, None)?.await?;
    let mut actions = stored
        .into_iter()
        .map(|val| {
            serde_wasm_bindgen::from_value(val)
                .map_err(|e| anyhow::anyhow!("unable to map action: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<PayeeAction>>>()?;

    transaction.await?;
    actions.sort_by_key(|a| a.sequence_number);
    Ok(actions)
}

/// Automatically purges any unconfirmed action whose creation time is > 1 hour old.
/// Returns the number of expired actions purged.
pub async fn purge_expired_actions(database: &Database) -> Result<usize, DatabaseError> {
    let now = Utc::now();
    let one_hour_ago = now - chrono::Duration::hours(1);

    let all_actions = get_all_actions(database).await?;
    let expired_ids: Vec<String> = all_actions
        .into_iter()
        .filter(|action| action.created_at < one_hour_ago && action.status != ActionStatus::Confirmed)
        .map(|a| a.id)
        .collect();

    if expired_ids.is_empty() {
        return Ok(0);
    }

    let count = expired_ids.len();
    let transaction = database.transaction(
        &[ACTION_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(ACTION_QUEUE_STORE_NAME)?;

    for id in expired_ids {
        store.delete(JsValue::from_str(&id))?.await?;
    }
    transaction.commit()?.await?;
    debug!("Purged {} expired actions older than 1 hour", count);

    Ok(count)
}

/// Fetches all pending / unconfirmed actions in sequence order after purging expired ones.
pub async fn get_pending_actions(database: &Database) -> Result<Vec<PayeeAction>, DatabaseError> {
    let _ = purge_expired_actions(database).await;
    let all = get_all_actions(database).await?;
    let pending: Vec<PayeeAction> = all
        .into_iter()
        .filter(|a| a.status != ActionStatus::Confirmed)
        .collect();
    Ok(pending)
}

pub async fn update_action(
    database: &Database,
    action: &PayeeAction,
) -> Result<(), DatabaseError> {
    enqueue_action(database, action).await
}

pub async fn delete_action(database: &Database, id: &str) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[ACTION_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(ACTION_QUEUE_STORE_NAME)?;
    store.delete(JsValue::from_str(id))?.await?;
    transaction.commit()?.await?;
    Ok(())
}

pub async fn clear_all_actions(database: &Database) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[ACTION_QUEUE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(ACTION_QUEUE_STORE_NAME)?;
    store.clear()?.await?;
    transaction.commit()?.await?;
    Ok(())
}

pub async fn get_next_action_sequence(database: &Database) -> Result<u64, DatabaseError> {
    let all = get_all_actions(database).await?;
    let max_seq = all.iter().map(|a| a.sequence_number).max().unwrap_or(0);
    Ok(max_seq + 1)
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(transparent)]
    IndexDbError(#[from] idb::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}
