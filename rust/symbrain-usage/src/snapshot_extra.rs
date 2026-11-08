//! Additive optional snapshot metadata: plan, reset grants, account, cache age.
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
