use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BudgetSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub last_modified_on: Option<String>,
    #[serde(default)]
    pub first_month: Option<String>,
    #[serde(default)]
    pub last_month: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BudgetsResponse {
    pub data: BudgetsResponseData,
}

#[derive(Debug, Deserialize)]
pub struct BudgetsResponseData {
    pub budgets: Vec<BudgetSummary>,
    #[serde(default)]
    pub default_budget: Option<BudgetSummary>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IgnoredPair {
    pub id: String,
    pub payee_id_1: String,
    pub payee_name_1: String,
    pub payee_id_2: String,
    pub payee_name_2: String,
    pub created_at: DateTime<Utc>,
}

impl IgnoredPair {
    pub fn make_id(id_a: &str, id_b: &str) -> String {
        if id_a <= id_b {
            format!("{}:{}", id_a, id_b)
        } else {
            format!("{}:{}", id_b, id_a)
        }
    }

    pub fn new(payee_1: &Payee, payee_2: &Payee) -> Self {
        let (p1, p2) = if payee_1.id <= payee_2.id {
            (payee_1, payee_2)
        } else {
            (payee_2, payee_1)
        };
        Self {
            id: format!("{}:{}", p1.id, p2.id),
            payee_id_1: p1.id.clone(),
            payee_name_1: p1.name.clone(),
            payee_id_2: p2.id.clone(),
            payee_name_2: p2.name.clone(),
            created_at: Utc::now(),
        }
    }
}
