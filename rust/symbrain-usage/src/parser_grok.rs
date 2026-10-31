use crate::{UsageError, UsageMeter, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;

// Only shared weekly quota is a meter. PAYG caps are not consumed credits;
// schema v1 has no badge/plan metadata, so do not invent a usage value.
pub(super) fn parse(body: &[u8], now: DateTime<Utc>) -> Result<UsageSnapshot, UsageError> {
    let invalid = || UsageError::payload("grok", "invalid credits config or period");
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let config = value
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let period = config
        .get("currentPeriod")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let date = |key: &str| {
        period
            .get(key)
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value.trim()).ok())
            .ok_or_else(invalid)
    };
    let start = date("start")?;
    let end = date("end")?;
    if end <= start {
        return Err(invalid());
    }
    if period.get("type").and_then(Value::as_str).map(str::trim) != Some("USAGE_PERIOD_TYPE_WEEKLY")
    {
        return Err(UsageError::payload(
            "grok",
            "weekly quota unavailable for this billing period (legacy monthly quota is not weekly)",
        ));
    }
    let used = config
        .get("creditUsagePercent")
        .map(|raw| {
            let number = match raw {
                Value::Number(value) => value.as_f64(),
                Value::String(value) => value.parse::<f64>().ok(),
                _ => None,
            }
            .filter(|value| value.is_finite())
            .ok_or_else(invalid)?;
            Ok::<_, UsageError>(super::fmt_num(number.clamp(0.0, 100.0)))
        })
        .transpose()?;
    Ok(UsageSnapshot {
        provider_id: "grok".into(),
        meters: vec![UsageMeter {
            label: "Weekly limit".into(),
            used,
            limit: Some("100".into()),
            unit: "%".into(),
            resets_at: Some(end),
        }],
        balance: None,
        currency: None,
        fetched_at: now,
        source: "api".into(),
    })
}

#[cfg(test)]
#[path = "parser_grok_tests.rs"]
mod tests;
