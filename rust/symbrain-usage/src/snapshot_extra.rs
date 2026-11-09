//! Additive optional snapshot metadata: plan, reset grants, organization billing.
//!
//! Every field of these types is optional on the wire, so a snapshot that
//! carries none of it serializes to the same bytes as before.
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::model::serialize_optional_go_timestamp;

/// Read-only status of Claude's one-off usage-limit reset grants. Nothing here
/// claims or consumes a grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetGrantStatus {
    pub eligible: bool,
    pub grants: Vec<ResetGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_grant_id: Option<String>,
}

/// Outcome of the organization billing lookup for an organization-billed
/// Copilot seat. Never a failure of the report: a member without billing
/// access gets `forbidden` and the rest of the report is unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgBilling {
    /// `available`, `forbidden` (HTTP 403: caller is not an organization owner
    /// or billing manager), `no_organization`, `no_copilot_usage` or `unavailable`.
    pub state: String,
    pub detail: String,
    /// Organization whose month-to-date totals the meter shows (`available` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

/// One reset grant: how many resets remain, until when, and what they refill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetGrant {
    pub id: String,
    pub resets_left: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_go_timestamp"
    )]
    pub ends_at: Option<DateTime<FixedOffset>>,
    pub clears: Vec<String>,
}
