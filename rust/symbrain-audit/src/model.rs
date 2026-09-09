use serde::{Deserialize, Serialize};

/// One JSONL audit record for a routed tool call or degradation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    pub profile: String,
    pub server: String,
    pub tool: String,
    pub duration_ms: i64,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub level: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arg_keys: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arg_values: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access_class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access_source: String,
}

/// Foreign-server exposure metadata copied into an audit entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exposure {
    pub access_class: String,
    pub access_source: String,
}

/// Failure category and retry guidance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Classification {
    pub category: String,
    pub retryable: bool,
}

/// Backend degradation from the latest gateway session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Degradation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,
    pub server: String,
    pub reason: String,
    pub level: String,
}

/// Audit logging switches from profile configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub verbose: bool,
}
