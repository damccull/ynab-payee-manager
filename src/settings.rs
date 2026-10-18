pub const REDIRECT_URL: &str = "http://localhost:8080"; // TODO: Replace this with config loaded from env/files at launch

pub const DATABASE_NAME: &str = "ynab-payee-manager";
pub const DATABASE_VERSION: u32 = 10;
pub const KNOWLEDGE_STORE_NAME: &str = "server_knowledge";
pub const PAYEES_STORE_NAME: &str = "payees";
pub const SETTINGS_STORE_NAME: &str = "settings";

pub const SERVER_KNOWLEDGE_KEY_PAYEES: &str = "payees";
pub const SETTINGS_KEY_TOKEN: &str = "ynab_user_token";
