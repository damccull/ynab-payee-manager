use std::collections::HashSet;
use reqwest::header::HeaderMap;

use crate::{
    database::{
        get_active_payee_ids, get_payees, get_transactions, replace_payees, replace_transactions,
    },
    models::{BudgetSummary, BudgetsResponse, Payee, Transaction, TransactionsResponse, YnabResponse},
};

pub async fn fetch_default_budget(token_str: &str) -> Result<BudgetSummary, String> {
    let httpclient = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    let auth_header = format!("Bearer {}", token_str)
        .parse()
        .map_err(|e| format!("Invalid auth header: {}", e))?;
    headers.insert("Authorization", auth_header);

    let url = "https://api.ynab.com/v1/budgets";

    let response = httpclient
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("YNAB API error {}: {}", status, body));
    }

    let budgets_resp: BudgetsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse budgets response: {}", e))?;

    if let Some(default_budget) = budgets_resp.data.default_budget {
        Ok(default_budget)
    } else if let Some(first_budget) = budgets_resp.data.budgets.into_iter().next() {
        Ok(first_budget)
    } else {
        Err("No budgets found in your YNAB account".to_string())
    }
}

pub async fn fetch_and_store_payees(
    db: &idb::Database,
    token_str: &str,
    budget: &str,
) -> Result<(Vec<Payee>, u64), String> {
    let httpclient = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    let auth_header = format!("Bearer {}", token_str)
        .parse()
        .map_err(|e| format!("Invalid auth header: {}", e))?;
    headers.insert("Authorization", auth_header);

    let url = format!("https://api.ynab.com/v1/budgets/{}/payees", budget);

    let response = httpclient
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("YNAB API error {}: {}", status, body));
    }

    let ynabresponse: YnabResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    replace_payees(db, &ynabresponse.data)
        .await
        .map_err(|e| format!("Failed to store payees in IndexedDB: {:?}", e))?;

    let (all_payees, k) = get_payees(db)
        .await
        .map_err(|e| format!("Failed reading payees from DB: {:?}", e))?;

    Ok((all_payees, k))
}

pub async fn fetch_and_store_transactions(
    db: &idb::Database,
    token_str: &str,
    budget: &str,
) -> Result<(Vec<Transaction>, u64, HashSet<String>), String> {
    let httpclient = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    let auth_header = format!("Bearer {}", token_str)
        .parse()
        .map_err(|e| format!("Invalid auth header: {}", e))?;
    headers.insert("Authorization", auth_header);

    let url = format!("https://api.ynab.com/v1/budgets/{}/transactions", budget);

    let response = httpclient
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("YNAB API error {}: {}", status, body));
    }

    let txresponse: TransactionsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    replace_transactions(db, &txresponse.data.transactions, txresponse.data.server_knowledge)
        .await
        .map_err(|e| format!("Failed to replace transactions: {:?}", e))?;

    let (all_txs, k) = get_transactions(db)
        .await
        .map_err(|e| format!("Failed reading transactions from DB: {:?}", e))?;

    let active_ids = get_active_payee_ids(db)
        .await
        .map_err(|e| format!("Failed computing active payee IDs: {:?}", e))?;

    Ok((all_txs, k, active_ids))
}
