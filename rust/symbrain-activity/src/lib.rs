//! Bounded, profile-gated activity read contracts.
//!
//! Raw capture payloads are deliberately absent. SQLite persistence remains
//! coupled to the memory-store port and is not implemented in this crate.

#![deny(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use symbrain_policy::{Profile, SERVER_MEMORY, evaluate_preset};

pub const MAX_RESULTS: usize = 50;
pub const MAX_TOKENS: usize = 4000;
pub const MAX_QUERY_LENGTH: usize = 512;
pub const MAX_RANGE_DAYS: i64 = 7;
pub const UNTRUSTED_FENCE_START: &str = "[UNTRUSTED_ACTIVITY_SUMMARY]";
pub const UNTRUSTED_FENCE_END: &str = "[/UNTRUSTED_ACTIVITY_SUMMARY]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityError {
    QueryRequired,
    QueryTooLong,
    WindowNotIncreasing,
    WindowTooLarge,
    InvalidLimit,
    InvalidTokenBudget,
}

impl fmt::Display for ActivityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueryRequired => write!(formatter, "activity query is required"),
            Self::QueryTooLong => write!(
                formatter,
                "activity query exceeds {MAX_QUERY_LENGTH} characters"
            ),
            Self::WindowNotIncreasing => write!(
                formatter,
                "activity query requires an increasing from/to window"
            ),
            Self::WindowTooLarge => write!(formatter, "activity query window exceeds 168h0m0s"),
            Self::InvalidLimit => write!(
                formatter,
                "activity query limit must be between 1 and {MAX_RESULTS}"
            ),
            Self::InvalidTokenBudget => write!(
                formatter,
                "activity query max_tokens must be between 1 and {MAX_TOKENS}"
            ),
        }
    }
}

impl std::error::Error for ActivityError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    pub query: String,
    pub source: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub limit: usize,
    pub max_tokens: usize,
    pub include_episodes: bool,
}

/// Validates the mandatory activity query bounds before storage access.
///
/// # Errors
/// Returns the same stable validation categories and messages as the Go API.
pub fn validate_search_options(options: &SearchOptions) -> Result<(), ActivityError> {
    if options.query.trim().is_empty() {
        return Err(ActivityError::QueryRequired);
    }
    if options.query.chars().count() > MAX_QUERY_LENGTH {
        return Err(ActivityError::QueryTooLong);
    }
    if options.to <= options.from {
        return Err(ActivityError::WindowNotIncreasing);
    }
    if options.to - options.from > Duration::days(MAX_RANGE_DAYS) {
        return Err(ActivityError::WindowTooLarge);
    }
    if !(1..=MAX_RESULTS).contains(&options.limit) {
        return Err(ActivityError::InvalidLimit);
    }
    if !(1..=MAX_TOKENS).contains(&options.max_tokens) {
        return Err(ActivityError::InvalidTokenBudget);
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prior_segment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<String>,
}

/// Safe consumer-facing activity shape containing redacted summary text only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadItem {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub granularity: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applications: Vec<String>,
    pub summary: String,
    pub provenance: Provenance,
    pub tokens: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPage {
    pub results: Vec<ReadItem>,
    pub truncated: bool,
    pub used_tokens: usize,
    pub max_tokens: usize,
}

/// Applies deterministic ordering, result count, and summary token bounds.
#[must_use]
pub fn bound_items(mut items: Vec<ReadItem>, limit: usize, max_tokens: usize) -> SearchPage {
    items.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let total = items.len();
    let mut page = SearchPage {
        results: Vec::with_capacity(total.min(limit)),
        truncated: false,
        used_tokens: 0,
        max_tokens,
    };
    for mut item in items {
        if page.results.len() >= limit {
            page.truncated = true;
            break;
        }
        let remaining = max_tokens.saturating_sub(page.used_tokens);
        if remaining == 0 {
            page.truncated = true;
            break;
        }
        item.summary = fit_summary(&item.summary, remaining);
        item.tokens = token_count(&item.summary);
        if item.tokens > remaining {
            page.truncated = true;
            break;
        }
        page.used_tokens += item.tokens;
        page.results.push(item);
    }
    if page.results.len() < total {
        page.truncated = true;
    }
    page
}

#[must_use]
pub fn fit_summary(summary: &str, max_tokens: usize) -> String {
    summary.chars().take(max_tokens.saturating_mul(4)).collect()
}

#[must_use]
pub fn token_count(value: &str) -> usize {
    if value.is_empty() {
        0
    } else {
        value.chars().count() / 4 + 1
    }
}

/// Wraps untrusted summary text and truncates the body to the CLI character budget.
#[must_use]
pub fn fence_summary(summary: &str, max_tokens: usize) -> String {
    let start = format!("{UNTRUSTED_FENCE_START}\n");
    let end = format!("\n{UNTRUSTED_FENCE_END}");
    let budget = max_tokens
        .saturating_mul(4)
        .saturating_sub(start.chars().count() + end.chars().count());
    let body = summary.chars().take(budget).collect::<String>();
    format!("{start}{body}{end}")
}

/// Returns whether the profile explicitly exposes at least one activity read tool.
#[must_use]
pub fn profile_allows_activity(profile: &Profile) -> bool {
    evaluate_preset(SERVER_MEMORY, &profile.server(SERVER_MEMORY)).is_ok_and(|report| {
        report.exposed.iter().any(|tool| {
            matches!(
                tool.as_str(),
                "activity_search" | "activity_get" | "activity_status"
            )
        })
    })
}

/// Generates the stable 32-hex-character activity identifier suffix used by Go.
#[must_use]
pub fn stable_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let hash = format!("{:x}", hasher.finalize());
    format!("activity-{}", &hash[..32])
}

#[must_use]
pub fn unique_sorted(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeZone;
    use symbrain_policy::{AuditConfig, ServerConfig};

    use super::*;

    fn options() -> SearchOptions {
        SearchOptions {
            query: "editor".to_string(),
            source: String::new(),
            from: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            to: Utc.with_ymd_and_hms(2026, 1, 1, 1, 0, 0).unwrap(),
            limit: 1,
            max_tokens: 100,
            include_episodes: false,
        }
    }

    #[test]
    fn validates_every_bound() {
        assert_eq!(validate_search_options(&options()), Ok(()));
        let mut invalid = options();
        invalid.query = " ".to_string();
        assert_eq!(
            validate_search_options(&invalid),
            Err(ActivityError::QueryRequired)
        );
        invalid = options();
        invalid.to = invalid.from;
        assert_eq!(
            validate_search_options(&invalid),
            Err(ActivityError::WindowNotIncreasing)
        );
    }

    #[test]
    fn unicode_fencing_and_token_count_match_go_runes() {
        assert_eq!(fit_summary("äöü😀abc", 1), "äöü😀");
        assert_eq!(token_count("abcd"), 2);
        let fenced = fence_summary("private", 20);
        assert!(fenced.starts_with(UNTRUSTED_FENCE_START));
        assert!(fenced.ends_with(UNTRUSTED_FENCE_END));
    }

    #[test]
    fn stable_id_and_unique_sort_are_deterministic() {
        assert_eq!(
            stable_id(&["segment", "source"]),
            stable_id(&["segment", "source"])
        );
        assert_eq!(
            unique_sorted(&[" b ".to_string(), "a".to_string(), "b".to_string()]),
            ["a", "b"]
        );
    }

    #[test]
    fn profile_access_requires_explicit_activity_tool() {
        let mut servers = BTreeMap::new();
        servers.insert(
            SERVER_MEMORY.to_string(),
            ServerConfig {
                enabled: true,
                mode: "read_only".to_string(),
                tools_allow: vec!["activity_status".to_string()],
                ..ServerConfig::default()
            },
        );
        let allowed = Profile {
            name: "activity".to_string(),
            description: String::new(),
            servers,
            audit: AuditConfig::default(),
            warnings: Vec::new(),
        };
        assert!(profile_allows_activity(&allowed));

        let denied = Profile {
            name: "restricted".to_string(),
            servers: BTreeMap::new(),
            ..allowed
        };
        assert!(!profile_allows_activity(&denied));
    }
}
