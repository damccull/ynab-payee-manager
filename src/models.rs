use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct YnabResponse {
    pub data: ResponseData,
}

#[derive(Debug, Deserialize)]
pub struct ResponseData {
    pub payees: Vec<Payee>,
    pub server_knowledge: u64,
}

#[derive(Debug, Deserialize)]
pub struct TransactionsResponse {
    pub data: TransactionsResponseData,
}

#[derive(Debug, Deserialize)]
pub struct TransactionsResponseData {
    pub transactions: Vec<Transaction>,
    pub server_knowledge: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Transaction {
    pub id: String,
    pub date: String,
    pub amount: i64,
    pub memo: Option<String>,
    pub cleared: String,
    pub approved: bool,
    pub payee_id: Option<String>,
    pub payee_name: Option<String>,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    #[serde(default)]
    pub transfer_account_id: Option<String>,
    #[serde(default)]
    pub transfer_transaction_id: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Payee {
    pub id: String,
    pub name: String,
    pub transfer_account_id: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PayeeReviewItem {
    pub id: String,
    pub suggested_name: String,
    pub custom_name: Option<String>,
    pub source_payee_ids: Vec<String>,
    pub source_payee_names: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl PayeeReviewItem {
    pub fn display_name(&self) -> &str {
        if let Some(custom) = &self.custom_name {
            if !custom.is_empty() {
                return custom;
            }
        }
        &self.suggested_name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionStatus {
    Pending,
    InProgress,
    Confirmed,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PayeeActionKind {
    /// Update an existing payee's name (PATCH /v1/budgets/{b}/payees/{id})
    RenamePayee {
        payee_id: String,
        old_name: String,
        new_name: String,
    },
    /// Reassign transactions from old payees to target payee to merge them
    MergePayees {
        target_payee_id: String,
        target_name: String,
        source_payee_ids: Vec<String>,
        source_payee_names: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PayeeAction {
    pub id: String,
    pub sequence_number: u64,
    pub kind: PayeeActionKind,
    pub status: ActionStatus,
    pub created_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PayeeCandidate {
    pub payee: Payee,
    pub is_checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateGroup {
    pub id: String,
    pub suggested_target_name: String,
    pub candidates: Vec<PayeeCandidate>,
    pub match_reason: String,
}
