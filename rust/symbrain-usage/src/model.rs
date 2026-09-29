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
    #[serde(serialize_with = "serialize_go_timestamp")]
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_go_timestamp"
    )]
    pub resets_at: Option<DateTime<chrono::FixedOffset>>,
}

fn format_go_rfc3339_nano(value: &DateTime<chrono::FixedOffset>) -> String {
    let mut encoded = value.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    if let Some(dot) = encoded.find('.') {
        let zone = if encoded.ends_with('Z') {
            encoded.len() - 1
        } else {
            encoded.len() - 6
        };
        let trimmed = encoded[dot..zone].trim_end_matches('0').len() + dot;
        if trimmed == dot + 1 {
            encoded.replace_range(dot..zone, "");
        } else {
            encoded.replace_range(trimmed..zone, "");
        }
    }
    encoded
}

fn serialize_go_timestamp<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format_go_rfc3339_nano(&value.fixed_offset()))
}

#[allow(clippy::ref_option)] // serde's serialize_with contract passes the field by reference.
fn serialize_optional_go_timestamp<S>(
    value: &Option<DateTime<chrono::FixedOffset>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&format_go_rfc3339_nano(value)),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::{UsageMeter, UsageSnapshot};
    use chrono::{DateTime, Utc};

    #[test]
    fn report_timestamps_trim_fractional_zeroes_like_go_rfc3339nano() {
        let timestamp: DateTime<Utc> = "2026-01-09T15:23:13.716839300Z"
            .parse()
            .expect("valid timestamp");
        let snapshot = UsageSnapshot {
            fetched_at: timestamp,
            ..UsageSnapshot::default()
        };
        let meter = UsageMeter {
            resets_at: Some(timestamp.fixed_offset()),
            ..UsageMeter::default()
        };
        let snapshot_json = serde_json::to_value(snapshot).expect("serialize snapshot");
        let meter_json = serde_json::to_value(meter).expect("serialize meter");
        assert_eq!(snapshot_json["fetched_at"], "2026-01-09T15:23:13.7168393Z");
        assert_eq!(meter_json["resets_at"], "2026-01-09T15:23:13.7168393Z");
        let offset: chrono::DateTime<chrono::FixedOffset> = "2026-01-09T17:53:13.716839300+02:30"
            .parse()
            .expect("valid offset timestamp");
        let offset_meter = UsageMeter {
            resets_at: Some(offset),
            ..UsageMeter::default()
        };
        assert_eq!(
            serde_json::to_value(offset_meter).expect("serialize offset meter")["resets_at"],
            "2026-01-09T17:53:13.7168393+02:30"
        );
        let decoded: UsageSnapshot =
            serde_json::from_value(snapshot_json).expect("deserialize snapshot");
        assert_eq!(decoded.fetched_at, timestamp);
        let missing_reset: UsageMeter = serde_json::from_value(serde_json::json!({
            "label": "window",
            "unit": "requests"
        }))
        .expect("missing optional reset remains accepted");
        assert_eq!(missing_reset.resets_at, None);
    }
}
