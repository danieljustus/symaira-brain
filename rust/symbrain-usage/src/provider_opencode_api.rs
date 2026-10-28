//! Official Go quota path; failures never switch to a web account.
use super::{Arc, Cancellation, Transport, UsageError, UsageSnapshot, Utc};
use crate::{Request, UsageMeter};
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;

#[derive(Deserialize)]
struct Window {
    percent: Option<f64>,
    #[serde(rename = "resetsAt")]
    resets_at: Option<DateTime<FixedOffset>>,
}

const URL: &str = "https://opencode.ai/zen/go/v1/usage";

pub(super) fn fetch(
    key: &str,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
) -> Result<UsageSnapshot, UsageError> {
    let request = Request {
        provider_id: "opencode".into(),
        method: "GET".into(),
        url: URL.into(),
        headers: [
            ("Authorization".into(), format!("Bearer {key}")),
            ("Accept".into(), "application/json".into()),
        ]
        .into(),
        body: None,
    };
    let timeout = cancel
        .remaining()
        .unwrap_or(std::time::Duration::from_secs(15))
        .min(std::time::Duration::from_secs(15));
    if cancel.is_cancelled() || timeout.is_zero() {
        return Err(UsageError::exact("OpenCode Go request failed"));
    }
    let bounded = Cancellation::with_timeout(timeout);
    let response = transport
        .request_with_cancel(request, &bounded)
        .map_err(|_| UsageError::exact("OpenCode Go request failed"))?;
    if response.body.len() > crate::transport::MAX_RESPONSE_BYTES {
        return Err(UsageError::exact("OpenCode Go response unreadable"));
    }
    match response.status {
        200 => {}
        401 => return Err(UsageError::exact("OpenCode Go key was rejected")),
        403 => {
            return Err(UsageError::exact(
                "OpenCode Go subscription unavailable for this key",
            ));
        }
        _ => return Err(UsageError::exact("OpenCode Go usage API unavailable")),
    }
    let now = Utc::now();
    let reference = response
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("date"))
        .and_then(|(_, v)| DateTime::parse_from_rfc2822(v).ok())
        .map_or(now, |v| v.with_timezone(&Utc));
    parse(&response.body, now, reference)
}

pub(super) fn parse(
    body: &[u8],
    now: DateTime<Utc>,
    reference: DateTime<Utc>,
) -> Result<UsageSnapshot, UsageError> {
    let invalid = || UsageError::exact("OpenCode Go quota response invalid");
    let root: serde_json::Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let usage = root
        .get("usage")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid)?;
    let mut snapshot = UsageSnapshot {
        provider_id: "opencode".into(),
        source: "api".into(),
        fetched_at: now,
        ..UsageSnapshot::default()
    };
    for (name, label) in [
        ("rolling", "Session"),
        ("weekly", "Weekly"),
        ("monthly", "Monthly"),
    ] {
        let Some(raw) = usage.get(name).filter(|v| !v.is_null()) else {
            continue;
        };
        let mut window: Window = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
        let used = window.percent.map(|n| n.clamp(0.0, 100.0));
        if name == "rolling"
            && used == Some(0.0)
            && window.resets_at.is_some_and(|reset| {
                let seconds = reset.signed_duration_since(reference).num_milliseconds();
                (seconds - 18_000_000).abs() <= 2000
            })
        {
            window.resets_at = None;
        }
        if used.is_some() || window.resets_at.is_some() {
            snapshot.meters.push(UsageMeter {
                label: label.into(),
                used: used.map(|n| n.to_string()),
                limit: Some("100".into()),
                unit: "%".into(),
                resets_at: window.resets_at,
            });
        }
    }
    if snapshot.meters.is_empty() {
        return Err(invalid());
    }
    Ok(snapshot)
}

#[cfg(test)]
#[path = "opencode_api_tests.rs"]
mod tests;
