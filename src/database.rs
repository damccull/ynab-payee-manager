use std::collections::{HashMap, HashSet};

use chrono::Utc;
use dioxus::prelude::{debug, error, trace};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use serde::Serialize;
use serde_wasm_bindgen::{from_value, Serializer};
use wasm_bindgen::JsValue;

use crate::{
    models::{ActionStatus, IgnoredPair, Payee, PayeeAction, PayeeReviewItem, ResponseData, Transaction},
    settings::{
        ACTION_QUEUE_STORE_NAME, DATABASE_NAME, DATABASE_VERSION, IGNORED_PAIRS_STORE_NAME,
        KNOWLEDGE_STORE_NAME, PAYEES_STORE_NAME, PAYEE_REVIEW_QUEUE_STORE_NAME,
        SERVER_KNOWLEDGE_KEY_PAYEES, SERVER_KNOWLEDGE_KEY_TRANSACTIONS, SETTINGS_KEY_TOKEN,
        SETTINGS_STORE_NAME, TRANSACTIONS_STORE_NAME,
    },
    YnabToken,
};

pub fn budget_database_name(budget_id: &str) -> String {
    format!("ynab-budget-{}", budget_id)
}

pub async fn create_database_for_budget(budget_id: &str) -> Result<Database, DatabaseError> {
    let name = budget_database_name(budget_id);
    create_database_with_name(&name).await
}

pub async fn create_database() -> Result<Database, DatabaseError> {
    create_database_with_name(DATABASE_NAME).await
}

pub async fn create_database_with_name(name: &str) -> Result<Database, DatabaseError> {
    let factory = Factory::new()?;
    let mut open_request = factory
        .open(name, Some(DATABASE_VERSION))
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

        // Ensure 'transactions' object store exists
        if !existing_stores.contains(&TRANSACTIONS_STORE_NAME.to_string()) {
            let mut tx_params = ObjectStoreParams::new();
            tx_params.auto_increment(false);
            tx_params.key_path(Some(KeyPath::new_single("id")));

            let tx_store = database
                .create_object_store(TRANSACTIONS_STORE_NAME, tx_params)
                .map_err(|e| error!("unable to create transactions store: {:#?}", e))
                .expect("unable to create transactions store");

            let mut payee_id_idx = IndexParams::new();
            payee_id_idx.unique(false);
            let _ = tx_store
                .create_index(
                    "payee_id",
                    KeyPath::new_single("payee_id"),
                    Some(payee_id_idx),
                )
                .map_err(|e| error!("unable to create payee_id index: {:#?}", e));
        }

        // Ensure 'ignored_pairs' object store exists
        if !existing_stores.contains(&IGNORED_PAIRS_STORE_NAME.to_string()) {
            let mut ignored_params = ObjectStoreParams::new();
            ignored_params.auto_increment(false);
            ignored_params.key_path(Some(KeyPath::new_single("id")));

            let ignored_store = database
                .create_object_store(IGNORED_PAIRS_STORE_NAME, ignored_params)
                .map_err(|e| error!("unable to create ignored pairs store: {:#?}", e))
                .expect("unable to create ignored pairs store");

            let mut created_index = IndexParams::new();
            created_index.unique(false);
            let _ = ignored_store
                .create_index(
                    "created_at",
                    KeyPath::new_single("created_at"),
                    Some(created_index),
                )
                .map_err(|e| error!("unable to create created_at index on ignored pairs: {:#?}", e));
        }
    });

    Ok(open_request.await?)
}

pub async fn nuke_database(database: &Database) -> Result<(), DatabaseError> {
    database.close();
    let factory = Factory::new()?;
    factory.delete(DATABASE_NAME)?.await?;
    Ok(())
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

    let val = data
        .server_knowledge
        .serialize(&Serializer::json_compatible())
        .map_err(|e| anyhow::anyhow!("unable to serialize server_knowledge: {:#?}", e))?;

    let knowledge = kstore
        .put(&val, Some(&JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES)))?
        .await?;

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

    let knowledge = match kstore.get(JsValue::from_str(SERVER_KNOWLEDGE_KEY_PAYEES))?.await? {
        Some(k) => from_value::<u64>(k).unwrap_or(0),
        None => 0,
    };

    transaction.await?;

    Ok((stored_payees, knowledge))
}

pub async fn replace_transactions(
    database: &Database,
    txs: &[Transaction],
    server_knowledge: u64,
) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[TRANSACTIONS_STORE_NAME, KNOWLEDGE_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;

    let store = transaction.object_store(TRANSACTIONS_STORE_NAME)?;
    store.clear()?.await?;

    for tx in txs {
        if !tx.deleted {
            let val = tx
                .serialize(&Serializer::json_compatible())
                .map_err(|e| anyhow::anyhow!("unable to serialize transaction: {:#?}", e))?;
            store.add(&val, None)?.await?;
        }
    }

    let kstore = transaction.object_store(KNOWLEDGE_STORE_NAME)?;
    let val = server_knowledge
        .serialize(&Serializer::json_compatible())
        .map_err(|e| anyhow::anyhow!("unable to serialize server_knowledge: {:#?}", e))?;
    kstore.put(&val, Some(&JsValue::from_str(SERVER_KNOWLEDGE_KEY_TRANSACTIONS)))?.await?;

    transaction.commit()?.await?;
    Ok(())
}

pub async fn get_transactions(
    database: &Database,
) -> Result<(Vec<Transaction>, u64), DatabaseError> {
    let transaction = database.transaction(
        &[TRANSACTIONS_STORE_NAME, KNOWLEDGE_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;

    let store = transaction.object_store(TRANSACTIONS_STORE_NAME)?;
    let stored = store.get_all(None, None)?.await?;

    let txs = stored
        .into_iter()
        .map(|v| {
            serde_wasm_bindgen::from_value(v)
                .map_err(|e| anyhow::anyhow!("unable to map transaction: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<Transaction>>>()?;

    let kstore = transaction.object_store(KNOWLEDGE_STORE_NAME)?;
    let knowledge = match kstore.get(JsValue::from_str(SERVER_KNOWLEDGE_KEY_TRANSACTIONS))?.await? {
        Some(k) => from_value::<u64>(k).unwrap_or(0),
        None => 0,
    };

    Ok((txs, knowledge))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum TransferPairRole {
    #[default]
    None,
    First,
    Second,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProcessedTransaction {
    pub id: String,
    pub date: String,
    pub sort_date: String,
    pub pair_id: Option<String>,
    pub outflow_milliunits: Option<u64>,
    pub inflow_milliunits: Option<u64>,
    pub payee_name: Option<String>,
    pub category_name: Option<String>,
    pub memo: Option<String>,
    pub cleared: String,
    pub approved: bool,
    pub is_transfer: bool,
    pub pair_role: TransferPairRole,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PagedTransactions {
    pub items: Vec<ProcessedTransaction>,
    pub total_count: usize,
    pub current_page: usize,
    pub page_size: usize,
    pub total_pages: usize,
}

pub async fn get_transactions_count(database: &Database) -> Result<usize, DatabaseError> {
    let transaction = database.transaction(
        &[TRANSACTIONS_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;
    let store = transaction.object_store(TRANSACTIONS_STORE_NAME)?;
    let count = store.count(None)?.await?;
    Ok(count as usize)
}

pub fn process_and_pair_transactions(
    txs: impl IntoIterator<Item = Transaction>,
) -> Vec<ProcessedTransaction> {
    let active_txs: Vec<Transaction> = txs.into_iter().filter(|tx| !tx.deleted).collect();

    // Map id -> index in active_txs
    let mut id_map: HashMap<&str, usize> = HashMap::with_capacity(active_txs.len());
    for (idx, tx) in active_txs.iter().enumerate() {
        id_map.insert(tx.id.as_str(), idx);
    }

    let mut paired_indices: HashSet<usize> = HashSet::with_capacity(active_txs.len());
    let mut processed = Vec::with_capacity(active_txs.len());

    for (idx, tx) in active_txs.iter().enumerate() {
        if paired_indices.contains(&idx) {
            continue;
        }

        // Check if this transaction has a transfer partner
        if let Some(ref partner_id) = tx.transfer_transaction_id {
            if !partner_id.is_empty() && partner_id != &tx.id {
                if let Some(&partner_idx) = id_map.get(partner_id.as_str()) {
                    if partner_idx != idx && !paired_indices.contains(&partner_idx) {
                        let partner = &active_txs[partner_idx];
                        paired_indices.insert(idx);
                        paired_indices.insert(partner_idx);

                        // Designate outflow (< 0) as First, and inflow (> 0) as Second
                        let (tx_out, tx_in) = if tx.amount <= partner.amount {
                            (tx, partner)
                        } else {
                            (partner, tx)
                        };

                        let group_date = if tx_out.date >= tx_in.date {
                            tx_out.date.clone()
                        } else {
                            tx_in.date.clone()
                        };

                        let pair_id = if tx_out.id <= tx_in.id {
                            format!("{}:{}", tx_out.id, tx_in.id)
                        } else {
                            format!("{}:{}", tx_in.id, tx_out.id)
                        };

                        let out_amount = tx_out.amount.abs() as u64;
                        let in_amount = tx_in.amount.abs() as u64;

                        // Outflow line (First)
                        processed.push(ProcessedTransaction {
                            id: tx_out.id.clone(),
                            date: tx_out.date.clone(),
                            sort_date: group_date.clone(),
                            pair_id: Some(pair_id.clone()),
                            outflow_milliunits: if out_amount > 0 { Some(out_amount) } else { None },
                            inflow_milliunits: None,
                            payee_name: None,
                            category_name: None,
                            memo: tx_out.memo.clone(),
                            cleared: tx_out.cleared.clone(),
                            approved: tx_out.approved,
                            is_transfer: true,
                            pair_role: TransferPairRole::First,
                        });

                        // Inflow line (Second)
                        processed.push(ProcessedTransaction {
                            id: tx_in.id.clone(),
                            date: tx_in.date.clone(),
                            sort_date: group_date,
                            pair_id: Some(pair_id),
                            outflow_milliunits: None,
                            inflow_milliunits: if in_amount > 0 { Some(in_amount) } else { None },
                            payee_name: None,
                            category_name: None,
                            memo: tx_in.memo.clone(),
                            cleared: tx_in.cleared.clone(),
                            approved: tx_in.approved,
                            is_transfer: true,
                            pair_role: TransferPairRole::Second,
                        });

                        continue;
                    }
                }
            }
        }

        // Single transaction (non-transfer or unpaired transfer)
        let is_transfer = tx.transfer_account_id.is_some() || tx.transfer_transaction_id.is_some();
        let (outflow, inflow) = if tx.amount < 0 {
            (Some(tx.amount.abs() as u64), None)
        } else if tx.amount > 0 {
            (None, Some(tx.amount as u64))
        } else {
            (None, None)
        };

        processed.push(ProcessedTransaction {
            id: tx.id.clone(),
            date: tx.date.clone(),
            sort_date: tx.date.clone(),
            pair_id: None,
            outflow_milliunits: outflow,
            inflow_milliunits: inflow,
            payee_name: if is_transfer { None } else { tx.payee_name.clone() },
            category_name: if is_transfer { None } else { tx.category_name.clone() },
            memo: tx.memo.clone(),
            cleared: tx.cleared.clone(),
            approved: tx.approved,
            is_transfer,
            pair_role: TransferPairRole::None,
        });
    }

    processed
}

pub fn filter_and_page_transactions(
    txs: impl IntoIterator<Item = Transaction>,
    query: &str,
    page: usize,
    page_size: usize,
) -> PagedTransactions {
    let query_trimmed = query.trim().to_lowercase();
    let has_query = !query_trimmed.is_empty();

    let processed = process_and_pair_transactions(txs);

    // Identify matched pair IDs so paired transactions stay together even if only one side matched query
    let matched_pair_ids: HashSet<String> = if has_query {
        processed
            .iter()
            .filter(|tx| {
                tx.date.to_lowercase().contains(&query_trimmed)
                    || tx.payee_name.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                    || tx.category_name.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                    || tx.memo.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                    || (tx.is_transfer && ("transfer".contains(&query_trimmed) || "account-to-account".contains(&query_trimmed)))
                    || tx.outflow_milliunits.is_some_and(|m| format!("{:.2}", (m as f64) / 1000.0).contains(&query_trimmed))
                    || tx.inflow_milliunits.is_some_and(|m| format!("{:.2}", (m as f64) / 1000.0).contains(&query_trimmed))
                    || tx.cleared.to_lowercase().contains(&query_trimmed)
            })
            .filter_map(|tx| tx.pair_id.clone())
            .collect()
    } else {
        HashSet::new()
    };

    let mut filtered: Vec<ProcessedTransaction> = processed
        .into_iter()
        .filter(|tx| {
            if !has_query {
                return true;
            }
            if let Some(ref pid) = tx.pair_id {
                if matched_pair_ids.contains(pid) {
                    return true;
                }
            }
            tx.date.to_lowercase().contains(&query_trimmed)
                || tx.payee_name.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                || tx.category_name.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                || tx.memo.as_deref().unwrap_or("").to_lowercase().contains(&query_trimmed)
                || (tx.is_transfer && ("transfer".contains(&query_trimmed) || "account-to-account".contains(&query_trimmed)))
                || tx.outflow_milliunits.is_some_and(|m| format!("{:.2}", (m as f64) / 1000.0).contains(&query_trimmed))
                || tx.inflow_milliunits.is_some_and(|m| format!("{:.2}", (m as f64) / 1000.0).contains(&query_trimmed))
                || tx.cleared.to_lowercase().contains(&query_trimmed)
        })
        .collect();

    // Sort newest to oldest, keeping transfer pairs adjacent and First before Second
    filtered.sort_by(|a, b| {
        b.sort_date
            .cmp(&a.sort_date)
            .then_with(|| {
                let a_group = a.pair_id.as_deref().unwrap_or(&a.id);
                let b_group = b.pair_id.as_deref().unwrap_or(&b.id);
                b_group.cmp(a_group)
            })
            .then_with(|| match (a.pair_role, b.pair_role) {
                (TransferPairRole::First, TransferPairRole::Second) => std::cmp::Ordering::Less,
                (TransferPairRole::Second, TransferPairRole::First) => std::cmp::Ordering::Greater,
                _ => b.id.cmp(&a.id),
            })
    });

    let total_count = filtered.len();
    let total_pages = if total_count == 0 {
        1
    } else {
        (total_count + page_size - 1) / page_size
    };
    let current_page = page.min(total_pages.saturating_sub(1));
    let start = current_page * page_size;
    let end = (start + page_size).min(total_count);

    let items = if start < end {
        filtered[start..end].to_vec()
    } else {
        Vec::new()
    };

    PagedTransactions {
        items,
        total_count,
        current_page,
        page_size,
        total_pages,
    }
}

pub async fn search_transactions_paged(
    database: &Database,
    query: &str,
    page: usize,
    page_size: usize,
) -> Result<PagedTransactions, DatabaseError> {
    let transaction = database.transaction(
        &[TRANSACTIONS_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;

    let store = transaction.object_store(TRANSACTIONS_STORE_NAME)?;
    let stored = store.get_all(None, None)?.await?;

    let txs = stored
        .into_iter()
        .filter_map(|v| serde_wasm_bindgen::from_value::<Transaction>(v).ok());

    let paged = filter_and_page_transactions(txs, query, page, page_size);

    Ok(paged)
}

pub async fn get_active_payee_ids(
    database: &Database,
) -> Result<HashSet<String>, DatabaseError> {
    let (txs, _) = get_transactions(database).await?;
    let mut set = HashSet::new();
    for tx in txs {
        if !tx.deleted {
            if let Some(pid) = tx.payee_id {
                if !pid.trim().is_empty() {
                    set.insert(pid);
                }
            }
        }
    }
    Ok(set)
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

// ----------------------------------------------------------------------------
// Ignored Duplicate Pairs Database Operations
// ----------------------------------------------------------------------------

pub async fn save_ignored_pairs(
    database: &Database,
    pairs: &[IgnoredPair],
) -> Result<(), DatabaseError> {
    if pairs.is_empty() {
        return Ok(());
    }
    let transaction = database.transaction(
        &[IGNORED_PAIRS_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(IGNORED_PAIRS_STORE_NAME)?;

    for pair in pairs {
        let js_val = pair
            .serialize(&Serializer::json_compatible())
            .map_err(|e| anyhow::anyhow!("unable to serialize ignored pair: {:#?}", e))?;
        store.put(&js_val, None)?.await?;
    }
    transaction.commit()?.await?;
    Ok(())
}

pub async fn get_ignored_pairs(database: &Database) -> Result<Vec<IgnoredPair>, DatabaseError> {
    let transaction = database.transaction(
        &[IGNORED_PAIRS_STORE_NAME],
        idb::TransactionMode::ReadOnly,
    )?;
    let store = transaction.object_store(IGNORED_PAIRS_STORE_NAME)?;

    let stored = store.get_all(None, None)?.await?;
    let mut items = stored
        .into_iter()
        .map(|val| {
            serde_wasm_bindgen::from_value(val)
                .map_err(|e| anyhow::anyhow!("unable to map ignored pair: {:#?}", e))
        })
        .collect::<anyhow::Result<Vec<IgnoredPair>>>()?;

    transaction.await?;

    // Sort newest first
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(items)
}

pub async fn get_ignored_pair_ids(database: &Database) -> Result<HashSet<String>, DatabaseError> {
    let pairs = get_ignored_pairs(database).await?;
    Ok(pairs.into_iter().map(|p| p.id).collect())
}

pub async fn delete_ignored_pair(database: &Database, pair_id: &str) -> Result<(), DatabaseError> {
    let transaction = database.transaction(
        &[IGNORED_PAIRS_STORE_NAME],
        idb::TransactionMode::ReadWrite,
    )?;
    let store = transaction.object_store(IGNORED_PAIRS_STORE_NAME)?;
    store.delete(JsValue::from_str(pair_id))?.await?;
    transaction.commit()?.await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(transparent)]
    IndexDbError(#[from] idb::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Transaction;

    fn make_test_tx(id: &str, date: &str, payee: &str, amount: i64, memo: &str) -> Transaction {
        Transaction {
            id: id.to_string(),
            date: date.to_string(),
            amount,
            memo: Some(memo.to_string()),
            cleared: "cleared".to_string(),
            approved: true,
            payee_id: Some(format!("p_{}", id)),
            payee_name: Some(payee.to_string()),
            category_id: None,
            category_name: Some("Dining".to_string()),
            transfer_account_id: None,
            transfer_transaction_id: None,
            deleted: false,
        }
    }

    #[test]
    fn test_filter_and_page_empty() {
        let paged = filter_and_page_transactions(Vec::<Transaction>::new(), "", 0, 10);
        assert_eq!(paged.total_count, 0);
        assert_eq!(paged.total_pages, 1);
        assert_eq!(paged.current_page, 0);
        assert!(paged.items.is_empty());
    }

    #[test]
    fn test_filter_and_page_sorting_and_paging() {
        let txs = vec![
            make_test_tx("1", "2024-01-01", "Coffee Shop", 5000, "Morning coffee"),
            make_test_tx("2", "2024-03-15", "Grocery Store", 75000, "Weekly groceries"),
            make_test_tx("3", "2024-02-10", "Bookstore", 25000, "Novel"),
            make_test_tx("4", "2024-04-01", "Gas Station", 45000, "Fuel"),
            make_test_tx("5", "2024-03-20", "Grocery Store", 32000, "Milk and bread"),
        ];

        // Page 0 with size 2 (should be newest first: 2024-04-01, 2024-03-20)
        let paged = filter_and_page_transactions(txs.clone(), "", 0, 2);
        assert_eq!(paged.total_count, 5);
        assert_eq!(paged.total_pages, 3);
        assert_eq!(paged.current_page, 0);
        assert_eq!(paged.items.len(), 2);
        assert_eq!(paged.items[0].date, "2024-04-01");
        assert_eq!(paged.items[1].date, "2024-03-20");

        // Page 1 with size 2 (should be 2024-03-15, 2024-02-10)
        let paged = filter_and_page_transactions(txs.clone(), "", 1, 2);
        assert_eq!(paged.current_page, 1);
        assert_eq!(paged.items.len(), 2);
        assert_eq!(paged.items[0].date, "2024-03-15");
        assert_eq!(paged.items[1].date, "2024-02-10");

        // Page 2 with size 2 (last page, 1 item: 2024-01-01)
        let paged = filter_and_page_transactions(txs.clone(), "", 2, 2);
        assert_eq!(paged.current_page, 2);
        assert_eq!(paged.items.len(), 1);
        assert_eq!(paged.items[0].date, "2024-01-01");
    }

    #[test]
    fn test_filter_by_search_query() {
        let txs = vec![
            make_test_tx("1", "2024-01-01", "Trader Joe's", 50000, "Snacks"),
            make_test_tx("2", "2024-01-02", "Safeway", 20000, "Produce"),
            make_test_tx("3", "2024-01-03", "Target", 15000, "Household"),
        ];

        let paged = filter_and_page_transactions(txs.clone(), "trader", 0, 10);
        assert_eq!(paged.total_count, 1);
        assert_eq!(paged.items[0].id, "1");

        let paged_memo = filter_and_page_transactions(txs.clone(), "produce", 0, 10);
        assert_eq!(paged_memo.total_count, 1);
        assert_eq!(paged_memo.items[0].id, "2");
    }

    #[test]
    fn test_deleted_transactions_excluded() {
        let tx1 = make_test_tx("1", "2024-01-01", "Active Payee", 10000, "");
        let mut tx2 = make_test_tx("2", "2024-01-02", "Deleted Payee", 20000, "");
        tx2.deleted = true;

        let paged = filter_and_page_transactions(vec![tx1, tx2], "", 0, 10);
        assert_eq!(paged.total_count, 1);
        assert_eq!(paged.items[0].id, "1");
    }

    #[test]
    fn test_process_and_pair_transfer_transactions() {
        let mut tx_out = make_test_tx("tx1", "2024-05-01", "Transfer : Savings", -100000, "Savings transfer");
        tx_out.transfer_account_id = Some("acc_savings".to_string());
        tx_out.transfer_transaction_id = Some("tx2".to_string());

        let mut tx_in = make_test_tx("tx2", "2024-05-01", "Transfer : Checking", 100000, "Savings transfer");
        tx_in.transfer_account_id = Some("acc_checking".to_string());
        tx_in.transfer_transaction_id = Some("tx1".to_string());

        let normal_tx = make_test_tx("tx3", "2024-05-02", "Grocery Store", -45000, "Food");

        let paged = filter_and_page_transactions(vec![tx_out, tx_in, normal_tx], "", 0, 10);
        // Both halves of the pair are kept as two separate lines, plus normal_tx = 3 total
        assert_eq!(paged.total_count, 3);

        // First item should be tx3 (2024-05-02)
        assert_eq!(paged.items[0].id, "tx3");
        assert_eq!(paged.items[0].is_transfer, false);
        assert_eq!(paged.items[0].pair_role, TransferPairRole::None);
        assert_eq!(paged.items[0].outflow_milliunits, Some(45000));
        assert_eq!(paged.items[0].inflow_milliunits, None);

        // Second item should be the Outflow side of the transfer (2024-05-01)
        assert_eq!(paged.items[1].id, "tx1");
        assert_eq!(paged.items[1].is_transfer, true);
        assert_eq!(paged.items[1].pair_role, TransferPairRole::First);
        assert_eq!(paged.items[1].outflow_milliunits, Some(100000));
        assert_eq!(paged.items[1].inflow_milliunits, None);
        assert_eq!(paged.items[1].memo.as_deref(), Some("Savings transfer"));

        // Third item should be the Inflow side of the transfer (2024-05-01), adjacent to the first
        assert_eq!(paged.items[2].id, "tx2");
        assert_eq!(paged.items[2].is_transfer, true);
        assert_eq!(paged.items[2].pair_role, TransferPairRole::Second);
        assert_eq!(paged.items[2].outflow_milliunits, None);
        assert_eq!(paged.items[2].inflow_milliunits, Some(100000));
        assert_eq!(paged.items[2].memo.as_deref(), Some("Savings transfer"));
    }

    #[test]
    fn test_search_transfer_unit() {
        let mut tx_out = make_test_tx("tx1", "2024-05-01", "Transfer : Savings", -100000, "");
        tx_out.transfer_transaction_id = Some("tx2".to_string());
        let mut tx_in = make_test_tx("tx2", "2024-05-01", "Transfer : Checking", 100000, "");
        tx_in.transfer_transaction_id = Some("tx1".to_string());

        let paged = filter_and_page_transactions(vec![tx_out.clone(), tx_in.clone()], "transfer", 0, 10);
        assert_eq!(paged.total_count, 2);
        assert_eq!(paged.items[0].is_transfer, true);
        assert_eq!(paged.items[0].pair_role, TransferPairRole::First);
        assert_eq!(paged.items[1].is_transfer, true);
        assert_eq!(paged.items[1].pair_role, TransferPairRole::Second);

        let paged_acct = filter_and_page_transactions(vec![tx_out, tx_in], "account-to-account", 0, 10);
        assert_eq!(paged_acct.total_count, 2);
    }
}
