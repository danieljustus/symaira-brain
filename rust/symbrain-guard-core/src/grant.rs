//! Enumerable, scoped, revocable grants.
//!
//! SEC-002 port of the authorization semantics of `guard/internal/grant`:
//! scope validity, the fail-closed binding check, the exact-match
//! authorization tuple, and the validation `grant.Store.Add` applies.
//! The persistent grants store and the `symguard grants` renderer live in
//! `symbrain-cli`; policy consults this module for standing grants.

use crate::go_time;
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

/// Originating approval layer for a grant.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Origin {
    /// Unix seconds of the originating decision.
    #[serde(default)]
    pub epoch: i64,
    /// Originating layer, e.g. `"approval"`; omitted when empty like Go's
    /// `json:"via,omitempty"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
}

/// A standing authorization bound to one subject.
///
/// Field order and `omitempty` semantics mirror the Go struct tags so
/// `to_go_json_vec` reproduces `encoding/json` output byte for byte.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Grant {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub origin: Origin,
    #[serde(default)]
    pub granted_at: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub capability: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resource: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_ceiling: Vec<String>,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revoked: bool,
}

fn scope_valid(scope: &str) -> bool {
    matches!(scope, "run" | "session" | "device" | "vault")
}

/// Mirrors Go's `time.IsZero`: January 1, year 1, 00:00:00 UTC.
pub(crate) fn is_go_zero_time(value: DateTime<FixedOffset>) -> bool {
    let zero = NaiveDate::from_ymd_opt(1, 1, 1)
        .expect("year 1 exists")
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists");
    value.naive_utc() == zero
}

impl Grant {
    /// Go's `bindingValid`: every binding field must be present and the
    /// expiry must be a parseable, non-zero instant.
    fn binding_valid(&self) -> bool {
        if self.capability.is_empty()
            || self.purpose.is_empty()
            || self.resource.is_empty()
            || self.expires_at.is_empty()
            || self.scope_ceiling.is_empty()
            || self.scope_ceiling.iter().any(String::is_empty)
        {
            return false;
        }
        match go_time::parse(self.expires_at.as_bytes()) {
            Ok(expires) => !is_go_zero_time(expires),
            Err(_) => false,
        }
    }

    /// Go's `Grant.Authorizes`: revoked, unknown scope, invalid binding,
    /// expired, or scope outside the ceiling all fail closed before the
    /// exact tuple match.
    #[must_use]
    pub fn authorizes(
        &self,
        capability: &str,
        purpose: &str,
        resource: &str,
        scope: &str,
        now: DateTime<FixedOffset>,
    ) -> bool {
        if self.revoked || !scope_valid(&self.scope) || !self.binding_valid() {
            return false;
        }
        let Ok(expires) = go_time::parse(self.expires_at.as_bytes()) else {
            return false;
        };
        if now >= expires {
            return false;
        }
        let ceiling_ok = self
            .scope_ceiling
            .iter()
            .any(|entry| entry == "*" || entry == scope);
        ceiling_ok
            && !scope.is_empty()
            && self.capability == capability
            && self.purpose == purpose
            && self.resource == resource
    }

    /// Mirrors `grant.Store.Add`'s fail-closed validation, in Go's order.
    ///
    /// # Errors
    /// Returns Go's diagnostic for a nil grant, empty ID, unknown scope,
    /// empty subject, or incomplete authorization binding.
    pub fn validate_add(grant: Option<&Grant>) -> Result<(), String> {
        let Some(grant) = grant else {
            return Err("grant: add nil grant".to_owned());
        };
        if grant.id.is_empty() {
            return Err("grant: add grant with empty ID".to_owned());
        }
        if !scope_valid(&grant.scope) {
            return Err(format!(
                "grant: add grant with unknown scope {:?}",
                grant.scope
            ));
        }
        if grant.subject.is_empty() {
            return Err("grant: add grant with empty subject".to_owned());
        }
        if !grant.binding_valid() {
            return Err("grant: add grant with incomplete authorization binding".to_owned());
        }
        Ok(())
    }
}
