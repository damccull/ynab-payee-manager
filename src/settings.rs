pub const REDIRECT_URL: &str = "http://localhost:8080"; // TODO: Replace this with config loaded from env/files at launch

pub const DATABASE_NAME: &str = "ynab-payee-manager";
pub const DATABASE_VERSION: u32 = 14;
pub const KNOWLEDGE_STORE_NAME: &str = "server_knowledge";
pub const PAYEES_STORE_NAME: &str = "payees";
pub const SETTINGS_STORE_NAME: &str = "settings";
pub const PAYEE_REVIEW_QUEUE_STORE_NAME: &str = "payee_review_queue";
pub const ACTION_QUEUE_STORE_NAME: &str = "action_queue";
pub const TRANSACTIONS_STORE_NAME: &str = "transactions";
pub const IGNORED_PAIRS_STORE_NAME: &str = "ignored_pairs";

pub const SERVER_KNOWLEDGE_KEY_PAYEES: &str = "payees";
pub const SERVER_KNOWLEDGE_KEY_TRANSACTIONS: &str = "transactions";
pub const SETTINGS_KEY_TOKEN: &str = "ynab_user_token";
