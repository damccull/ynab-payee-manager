use std::fmt::Display;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct YnabToken {
    raw: String,
    expires_at: DateTime<Utc>,
}

impl YnabToken {
    /// Creates a new [`YnabToken`] with a expiration sent by YNAB.
    /// This time limit is determined by the YNAB API.
    ///
    /// Note: See the "Implicit Grant Flow" section of <https://api.ynab.com/#oauth-applications>
    /// for more information
    ///
    /// Parameters:
    /// * **token**: _String_ - The opaque token string returned from the YNAB API
    ///
    /// Returns: YnabToken
    pub fn new(token: String) -> Self {
        let token_expires = Utc::now();
        // TODO: Update this to accept seconds after parsing the token data
        let expires_in = TimeDelta::hours(2);
        let token_expires = token_expires + expires_in;

        Self {
            raw: token,
            expires_at: token_expires,
        }
    }

    /// Whether this token has expired or not.
    ///
    /// Returns: bool
    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
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

impl Display for YnabToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw)
    }
}
