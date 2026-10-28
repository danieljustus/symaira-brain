//! Recorded local-machine usage; never cloud billing or current-account attribution.
use crate::Cancellation;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalHistory {
    pub scope: String,
    pub attribution: String,
    pub currency: String,
    pub cost_basis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_snapshot: Option<String>,
    pub timezone: String,
    pub status: String,
    pub excluded_events: usize,
    pub daily: Vec<HistoryBucket>,
    pub periods: Vec<HistoryBucket>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryBucket {
    pub key: String,
    pub models: Vec<HistoryModel>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryModel {
    pub model: String,
    pub events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_cost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_basis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_events: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_events: Option<usize>,
    pub missing_tokens: usize,
    pub missing_cost: usize,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Event {
    pub(crate) id: String,
    pub(crate) model: String,
    pub(crate) at: DateTime<Utc>,
    pub(crate) tokens: Option<u64>,
    pub(crate) ticks: Option<u64>,
}
fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|n| *n <= 1_000_000_000_000_000)
}
pub(crate) fn parse_line(line: &[u8]) -> Result<Vec<Event>, ()> {
    let obj: Value = serde_json::from_slice(line).map_err(|_| ())?;
    let env = obj.get("params").unwrap_or(&obj);
    let update = &env["update"];
    if update["sessionUpdate"] != "turn_completed" {
        return Ok(Vec::new());
    }
    let meta = &env["_meta"];
    let at = count(&meta["agentTimestampMs"])
        .filter(|n| *n > 0 && *n < 253_402_300_800_000)
        .and_then(|n| DateTime::from_timestamp_millis(i64::try_from(n).ok()?))
        .or_else(|| {
            obj["timestamp"]
                .as_str()
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        })
        .or_else(|| {
            count(&obj["timestamp"])
                .filter(|n| *n > 0 && *n < 253_402_300_800)
                .and_then(|n| DateTime::from_timestamp(i64::try_from(n).ok()?, 0))
        })
        .ok_or(())?;
    let id = meta["eventId"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(())?;
    let models = update["usage"]["modelUsage"]
        .as_object()
        .filter(|m| !m.is_empty() && m.len() <= 32)
        .ok_or(())?;
    let mut events = Vec::new();
    for (model, values) in models {
        if !model
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            || model.contains("..")
            || model.len() > 128
            || !model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
            || !values.is_object()
        {
            return Err(());
        }
        let tokens = count(&values["inputTokens"])
            .zip(count(&values["outputTokens"]))
            .filter(|(i, o)| *i <= 1_000_000_000_000 && *o <= 1_000_000_000_000)
            .map(|(i, o)| i + o);
        let ticks = if values.get("costUsdTicks").is_none() && models.len() == 1 {
            count(&update["usage"]["costUsdTicks"])
        } else {
            count(&values["costUsdTicks"])
        };
        events.push(Event {
            id: id.into(),
            model: model.clone(),
            at,
            tokens,
            ticks,
        });
    }
    Ok(events)
}

/// Scans bounded ledgers with an injected clock and calendar zone, without credentials.
#[must_use]
pub fn scan_grok_history<T: TimeZone>(
    home: &Path,
    now: &DateTime<T>,
    timezone: &str,
    cancel: &Cancellation,
) -> LocalHistory {
    let mut h = LocalHistory {
        scope: "local_machine".into(),
        attribution: "unattributed".into(),
        currency: "USD".into(),
        cost_basis: "recorded".into(),
        price_snapshot: None,
        timezone: timezone.into(),
        status: "no_data".into(),
        excluded_events: 0,
        daily: vec![],
        periods: vec![],
    };
    if !safe_home(home) {
        h.status = "unavailable".into();
        return h;
    }
    match crate::history_files::read(home, cancel) {
        Ok((events, excluded)) => {
            h.excluded_events = excluded;
            aggregate(&mut h, events, now);
        }
        Err(()) => h.status = "unavailable".into(),
    }
    h
}
pub(crate) fn collect_enabled(
    report: &mut crate::Report,
    cancelled: &impl Fn() -> bool,
    deadline: Option<std::time::Instant>,
    opencode_selected: bool,
) {
    for provider in &mut report.providers {
        if opencode_selected
            && provider.id == "opencode"
            && std::env::var("SYMBRAIN_USAGE_OPENCODE_HISTORY").as_deref() == Ok("1")
        {
            let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
            let base = std::env::var_os("XDG_DATA_HOME")
                .map_or_else(|| home.join(".local/share"), std::path::PathBuf::from);
            let dir = std::env::var_os("OPENCODE_DATA_DIR")
                .filter(|v| !v.to_string_lossy().trim().is_empty())
                .map_or_else(|| base.join("opencode"), std::path::PathBuf::from);
            let cancel = deadline.map_or_else(Cancellation::default, Cancellation::with_deadline);
            if cancelled() {
                cancel.cancel();
            }
            provider.history = Some(crate::providers::provider_config::scan_opencode_history(
                &dir,
                &chrono::Local::now(),
                "local",
                &cancel,
            ));
            continue;
        }
        if provider.id != "grok"
            || std::env::var("SYMBRAIN_USAGE_GROK_HISTORY").as_deref() != Ok("1")
        {
            continue;
        }
        let home = std::env::var_os("GROK_HOME")
            .filter(|v| !v.to_string_lossy().trim().is_empty())
            .map_or_else(
                || {
                    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".grok")
                },
                |v| std::path::PathBuf::from(v.to_string_lossy().trim()),
            );
        let cancel = deadline.map_or_else(Cancellation::default, Cancellation::with_deadline);
        if cancelled() {
            cancel.cancel();
        }
        provider.history = Some(scan_grok_history(
            &home,
            &chrono::Local::now(),
            "local",
            &cancel,
        ));
    }
}

fn safe_home(home: &Path) -> bool {
    home.is_absolute()
        && !home
            .as_os_str()
            .as_encoded_bytes()
            .split(|b| *b == b'/')
            .any(|c| c == b"." || c == b"..")
}
pub(crate) fn aggregate<T: TimeZone>(h: &mut LocalHistory, events: Vec<Event>, now: &DateTime<T>) {
    let mut unique = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for e in events {
        let key = (e.id.clone(), e.model.clone());
        if let Some(old) = unique.get(&key) {
            if old != &e {
                conflicts.insert(key);
            }
        } else {
            unique.insert(key, e);
        }
    }
    let today = now.date_naive();
    let since = today - Duration::days(29);
    let yesterday = today - Duration::days(1);
    let mut buckets: BTreeMap<String, BTreeMap<String, Vec<Event>>> = BTreeMap::new();
    for (key, e) in unique {
        if conflicts.contains(&key) {
            h.excluded_events += 1;
            continue;
        }
        let day = e.at.with_timezone(&now.timezone()).date_naive();
        if day < since || e.at > now.with_timezone(&Utc) {
            continue;
        }
        let mut keys = vec![format!("d:{day}"), "p:last_30_days".into()];
        if day == today {
            keys.push("p:today".into());
        }
        if day == yesterday {
            keys.push("p:yesterday".into());
        }
        for key in keys {
            buckets
                .entry(key)
                .or_default()
                .entry(e.model.clone())
                .or_default()
                .push(e.clone());
        }
    }
    for (key, models) in buckets {
        let b = HistoryBucket {
            key: key[2..].into(),
            models: models
                .into_iter()
                .map(|(m, events)| sum(m, &events))
                .collect(),
        };
        if key.starts_with("d:") {
            h.daily.push(b);
        } else {
            h.periods.push(b);
        }
    }
    if !h.daily.is_empty() {
        h.status = "available".into();
    }
    if h.excluded_events > 0
        || h.daily
            .iter()
            .flat_map(|b| &b.models)
            .any(|m| m.missing_tokens > 0 || m.missing_cost > 0)
    {
        h.status = "partial".into();
    }
}
fn sum(model: String, events: &[Event]) -> HistoryModel {
    let missing_tokens = events.iter().filter(|e| e.tokens.is_none()).count();
    let missing_cost = events.iter().filter(|e| e.ticks.is_none()).count();
    let tokens = (missing_tokens == 0).then(|| {
        events
            .iter()
            .filter_map(|e| e.tokens)
            .sum::<u64>()
            .to_string()
    });
    let recorded_cost = (missing_cost == 0).then(|| {
        let ticks = events.iter().filter_map(|e| e.ticks).sum::<u64>();
        let whole = ticks / 10_000_000_000;
        let rest = ticks % 10_000_000_000;
        if rest == 0 {
            whole.to_string()
        } else {
            format!("{whole}.{}", format!("{rest:010}").trim_end_matches('0'))
        }
    });
    HistoryModel {
        model,
        events: events.len(),
        tokens,
        recorded_cost,
        estimated_cost: None,
        cost_basis: None,
        recorded_events: None,
        estimated_events: None,
        missing_tokens,
        missing_cost,
    }
}
