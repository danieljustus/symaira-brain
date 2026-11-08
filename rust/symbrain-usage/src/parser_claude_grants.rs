//! Read-only parse of Claude's `cedar_ember` reset-grant block (#999).
//!
//! The block is trusted all or nothing: a missing, `null`, non-object or
//! partly malformed block yields no grant data at all, never a half-read one.
use super::{number, parse_time};
use crate::{ResetGrant, ResetGrantStatus};
use serde_json::Value;

pub(super) fn parse_reset_grants(block: Option<&Value>) -> Option<ResetGrantStatus> {
    let object = block?.as_object()?;
    let mut grants = Vec::new();
    if let Some(entries) = object.get("grants").and_then(Value::as_array) {
        for entry in entries {
            grants.push(parse_grant(entry)?);
        }
    }
    Some(ResetGrantStatus {
        eligible: object.get("eligible").and_then(Value::as_bool) == Some(true),
        grants,
        next_grant_id: object
            .get("next_grant_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
    })
}

fn parse_grant(entry: &Value) -> Option<ResetGrant> {
    let id = entry.get("id")?.as_str().filter(|id| !id.is_empty())?;
    let left = number(entry.get("resets_left")).filter(|value| value.is_finite())?;
    Some(ResetGrant {
        id: id.to_owned(),
        resets_left: whole_resets(left),
        ends_at: parse_time(entry.get("ends_at")),
        clears: entry
            .get("clears")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to >= 0, saturating.
fn whole_resets(value: f64) -> u64 {
    value.floor().max(0.0) as u64
}
