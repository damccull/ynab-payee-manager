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

#[derive(Debug, Deserialize, Serialize)]
pub struct Payee {
    pub id: String,
    pub name: String,
    pub transfer_account_id: Option<String>,
    pub deleted: bool,
}
