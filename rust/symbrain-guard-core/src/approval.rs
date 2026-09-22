//! Approval decisions converted into standing session grants.
//!
//! SEC-002 port of `approval.Decision.Grant` from
//! `guard/internal/approval`: an approved decision with a positive TTL
//! becomes a session grant whose authorization binding is copied verbatim.
//! Empty bindings are intentionally not wildcards; the policy engine fails
//! closed on them via [`crate::grant::Grant::authorizes`].

use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

use crate::grant::{Grant, Origin, is_go_zero_time};

/// The subset of `approval.Decision` the conversion reads. Unknown fields
/// (id, payload, reason) are ignored, mirroring a Go struct decode.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ApprovalDecision {
    #[serde(default)]
    pub approved: bool,
    /// Go `time.Duration` in nanoseconds (`json:"ttl"`).
    #[serde(default)]
    pub ttl: i64,
    /// Go `time.Time` as RFC3339 (`json:"decided_at"`).
    #[serde(default)]
    pub decided_at: String,
    #[serde(default)]
    pub capability: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub resource: String,
    #[serde(default)]
    pub scope_ceiling: Vec<String>,
}

/// Formats an instant the way Go marshals `time.Time` for whole-second
/// values (the corpus never carries fractional seconds).
fn format_go_time(value: DateTime<FixedOffset>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Converts an approved decision into a standing session grant.
///
/// The grant ID is an injected parameter: Go mints it from wall-clock
/// entropy (`grant.NewID`), which no fixture could freeze; every other
/// field is production output. A zero `decided_at` falls back to the
/// current instant, mirroring Go's `time.Now()` fallback — the oracle
/// corpus always supplies a non-zero `decided_at`.
///
/// # Errors
/// Returns Go's fail-closed diagnostic when the subject is empty, or the
/// time parser's diagnostic for an unparseable `decided_at`.
pub fn grant_from_decision(
    decision: &ApprovalDecision,
    subject: &str,
    id: &str,
) -> Result<Option<Grant>, String> {
    if decision.ttl <= 0 || !decision.approved {
        return Ok(None);
    }
    if subject.is_empty() {
        return Err("approval: grant subject is empty".to_owned());
    }
    // Preserve the input's numeric zone like Go's `time.Time` does;
    // go_time::parse normalizes to UTC, which would lose the offset that
    // encoding/json prints back. Fall back to it for validation errors
    // (its diagnostics are the pinned Go ones) and for the rare input
    // forms chrono rejects but Go accepts (comma fractions).
    let parsed = match DateTime::parse_from_rfc3339(&decision.decided_at) {
        Ok(value) => value,
        Err(_) => crate::go_time::parse(decision.decided_at.as_bytes())?,
    };
    let decided = if is_go_zero_time(parsed) {
        Utc::now().fixed_offset()
    } else {
        parsed
    };
    let expires = decided + chrono::Duration::nanoseconds(decision.ttl);
    Ok(Some(Grant {
        id: id.to_owned(),
        scope: "session".to_owned(),
        origin: Origin {
            epoch: decided.timestamp(),
            via: "approval".to_owned(),
        },
        granted_at: format_go_time(decided),
        subject: subject.to_owned(),
        capability: decision.capability.clone(),
        purpose: decision.purpose.clone(),
        resource: decision.resource.clone(),
        scope_ceiling: decision.scope_ceiling.clone(),
        expires_at: format_go_time(expires),
        revoked: false,
    }))
}
