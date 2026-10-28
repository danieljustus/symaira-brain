//! Undocumented coding-plan quota endpoints; never infer account or plan limits.
use crate::{UsageError, UsageMeter, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
#[path = "parser_devin.rs"]
mod devin;

fn numeric(v: Option<&Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|n| n.is_finite())
}
fn reset(v: Option<&Value>, millis: bool) -> Result<Option<DateTime<chrono::FixedOffset>>, ()> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let n = numeric(Some(v)).ok_or(())?;
    if millis {
        if !(0.0..=253_402_300_799_000.0).contains(&n) {
            return Err(());
        }
        let ms: i64 = n.trunc().to_string().parse().map_err(|_| ())?;
        return DateTime::from_timestamp_millis(ms)
            .map(|t| Some(t.fixed_offset()))
            .ok_or(());
    }
    if !(0.0..=253_402_300_799.0).contains(&n) {
        return Err(());
    }
    // Decimal parsing avoids unchecked float-to-integer overflow.
    let seconds: i64 = n.floor().to_string().parse().map_err(|_| ())?;
    let nanos: u32 = ((n - n.floor()) * 1e9)
        .trunc()
        .to_string()
        .parse()
        .map_err(|_| ())?;
    DateTime::from_timestamp(seconds, nanos)
        .map(|t| Some(t.fixed_offset()))
        .ok_or(())
}
pub(super) fn parse(
    id: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<UsageSnapshot, UsageError> {
    let invalid = || UsageError::parse(id, "quota data unavailable");
    let root: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let mut s = UsageSnapshot {
        provider_id: id.into(),
        fetched_at: now,
        source: "api".into(),
        ..UsageSnapshot::default()
    };
    match id {
        "zai" => zai(&root, &mut s),
        "devin" => devin::parse(&root, &mut s),
        _ => Err(()),
    }
    .map_err(|()| invalid())?;
    Ok(s)
}
fn zai(root: &Value, s: &mut UsageSnapshot) -> Result<(), ()> {
    if root.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(());
    }
    let container = root.get("data").unwrap_or(root);
    let entries = container
        .get("limits")
        .and_then(Value::as_array)
        .ok_or(())?;
    for e in entries {
        if !e.is_object() {
            return Err(());
        }
        let kind = e
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| e.get("name").and_then(Value::as_str));
        let (label, used, limit, unit) = match kind {
            Some("CREDIT_LIMIT" | "TOKENS_LIMIT") => {
                let u = numeric(e.get("unit")).ok_or(())?;
                let n = numeric(e.get("number")).filter(|n| *n > 0.0).ok_or(())?;
                let hours = match u {
                    3.0 => 1.0,
                    4.0 => 24.0,
                    5.0 => 720.0,
                    6.0 => 168.0,
                    _ => continue,
                } * n;
                if !hours.is_finite() || !(1.0 / 3_600_000.0..=2.56e12).contains(&hours) {
                    return Err(());
                }
                let used = numeric(e.get("percentage")).ok_or(())?.clamp(0.0, 100.0);
                (
                    if hours < 24.0 { "Session" } else { "Weekly" },
                    used,
                    100.0,
                    "%",
                )
            }
            Some("TIME_LIMIT") => {
                let used = numeric(e.get("currentValue"))
                    .filter(|n| *n >= 0.0)
                    .ok_or(())?;
                let limit = numeric(e.get("usage")).filter(|n| *n >= 0.0).ok_or(())?;
                ("Web Searches", used, limit, "searches")
            }
            _ => continue,
        };
        s.meters.push(UsageMeter {
            label: label.into(),
            used: Some(used.to_string()),
            limit: Some(limit.to_string()),
            unit: unit.into(),
            resets_at: reset(e.get("nextResetTime"), true)?,
        });
    }
    Ok(())
}
