use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YnabToken {
    raw: String,
    expires_at: DateTime<Utc>,
}

impl YnabToken {
    pub fn new(token: String) -> Self {
        let token_expires = Utc::now();
        let two_hours = TimeDelta::hours(2);
        let token_expires = token_expires + two_hours;

        Self {
            raw: token,
            expires_at: token_expires,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}

impl From<&str> for YnabToken {
    fn from(value: &str) -> Self {
        YnabToken::new(value.to_string())
    }
}
impl From<String> for YnabToken {
    fn from(value: String) -> Self {
        YnabToken::new(value)
    }
}
