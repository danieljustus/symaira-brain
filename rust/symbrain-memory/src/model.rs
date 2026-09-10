use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Sql(rusqlite::Error),
    Invalid(String),
    Cancelled,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) | Self::Invalid(error) => formatter.write_str(error),
            Self::Sql(error) => write!(formatter, "{error}"),
            Self::Cancelled => formatter.write_str("operation cancelled"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub scope: String,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_session: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub consolidation_status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub importance: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[derive(Default)]
pub struct SetOptions {
    pub session_id: String,
    pub entities: Vec<String>,
    pub working: bool,
    pub staged: bool,
}

pub struct Store {
    pub(crate) conn: Mutex<Connection>,
}
