use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{REPORT_SCHEMA_VERSION, providers::Provider};

/// Credential resolution state exposed without credential values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub status: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Stable schema-v1 usage report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub providers: Vec<ProviderUsage>,
}

impl Report {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            providers: Vec::new(),
        }
    }
}

/// One provider's auth state and optional read result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub id: String,
    pub display_name: String,
    pub configured: bool,
    pub auth_status: AuthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<UsageSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ProviderUsage {
    pub(crate) fn from_provider(provider: &Provider) -> Self {
        Self {
            id: provider.id.clone(),
            display_name: provider.display_name.clone(),
            configured: provider.configured,
            auth_status: provider.auth_status.clone(),
            snapshot: None,
            error: None,
        }
    }
}

/// Normalized read from one provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub provider_id: String,
    pub meters: Vec<UsageMeter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub source: String,
}

/// One normalized quota or balance meter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageMeter {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}
