//! Bounded Claude Code completed records and offline, source-pinned API-rate estimates.
use crate::{
    Cancellation,
    history::{Event, LocalHistory},
};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Deserialize)]
struct Rate {
    input: u64,
    output: u64,
    write: u64,
    read: u64,
    input_long: Option<u64>,
    output_long: Option<u64>,
    write_long: Option<u64>,
    read_long: Option<u64>,
    fast: Option<u64>,
}
#[derive(Deserialize)]
struct Prices {
    snapshot: String,
    rates: BTreeMap<String, Rate>,
    aliases: BTreeMap<String, String>,
}
#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) event: Event,
    message: String,
    request: String,
    side: bool,
    speed: bool,
    unsupported: bool,
    pub(crate) estimate: Option<u64>,
}
fn count(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n <= 1_000_000_000_000)
}
fn bucket(u: &Value, name: &str, optional: bool) -> Option<u64> {
    if optional && u.get(name).is_none() {
        Some(0)
    } else {
        count(&u[name])
    }
}
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.contains(['\0', '\r', '\n'])
}
#[derive(Deserialize)]
struct Cost<'a> {
    #[serde(borrow, rename = "costUSD")]
    cost: Option<&'a serde_json::value::RawValue>,
}
fn model_id(m: &Value) -> Result<&str, ()> {
    m["model"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && !s.contains("..")
                && s.as_bytes()[0].is_ascii_alphanumeric()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
        })
        .ok_or(())
}
fn persisted_usage(m: &Value) -> Result<bool, ()> {
    if m.get("stop_reason")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        return Err(());
    }
    match m["stop_reason"].as_str() {
        None | Some("") => {
            // Claude Code persists assistant usage without a turn-final stop marker.
            if m["usage"].is_null() {
                return Ok(false);
            }
            let u = m["usage"].as_object().ok_or(())?;
            if u.is_empty() {
                return Ok(false);
            }
            for key in ["input_tokens", "output_tokens"] {
                m["usage"][key]
                    .as_u64()
                    .filter(|n| *n <= 1_000_000_000_000_000)
                    .ok_or(())?;
            }
            Ok(true)
        }
        Some("end_turn" | "tool_use" | "max_tokens" | "stop_sequence") => Ok(true),
        _ => Err(()),
    }
}
fn parse(line: &[u8], prices: &Prices) -> Result<Option<Entry>, ()> {
    let obj: Value = serde_json::from_slice(line).map_err(|_| ())?;
    if obj["type"] != "assistant" {
        return Ok(None);
    }
    let m = &obj["message"];
    if m["role"] != "assistant" {
        return Err(());
    }
    if !persisted_usage(m)? {
        return Ok(None);
    }
    let at = obj["timestamp"]
        .as_str()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .ok_or(())?;
    let message = m["id"].as_str().filter(|s| valid_id(s)).ok_or(())?;
    let request = obj["requestId"]
        .as_str()
        .filter(|s| valid_id(s))
        .ok_or(())?;
    let model = model_id(m)?;
    let side = match obj.get("isSidechain") {
        None => false,
        Some(v) => v.as_bool().ok_or(())?,
    };
    let cost: Cost<'_> = serde_json::from_slice(line).map_err(|_| ())?;
    let ticks = cost
        .cost
        .and_then(|v| crate::providers::provider_config::opencode_recorded_ticks(v.get()));
    let u = &m["usage"];
    let input = bucket(u, "input_tokens", false);
    let output = bucket(u, "output_tokens", false);
    let read = bucket(u, "cache_read_input_tokens", true);
    let mut write = bucket(u, "cache_creation_input_tokens", true);
    let mut write1 = Some(0);
    if let Some(split) = u.get("cache_creation") {
        write = count(&split["ephemeral_5m_input_tokens"]);
        write1 = count(&split["ephemeral_1h_input_tokens"]);
        if let Some(aggregate) = u.get("cache_creation_input_tokens")
            && count(aggregate) != write.zip(write1).map(|(a, b)| a + b)
        {
            write = None;
        }
    }
    let speed = u.get("speed");
    let fast = speed.is_some_and(|v| v == "fast");
    let mut unsupported = speed.is_some_and(|v| v != "fast" && v != "standard")
        || u.get("service_tier").is_some_and(|v| v != "standard");
    if let Some(iterations) = u.get("iterations") {
        unsupported |= iterations
            .as_array()
            .is_none_or(|a| a.iter().any(|v| v["type"] != "message"));
    }
    let buckets = input
        .zip(output)
        .zip(read)
        .zip(write)
        .zip(write1)
        .map(|((((i, o), r), w), h)| [i, o, r, w, h]);
    let tokens = buckets.map(|b| b.iter().sum());
    let estimate = if obj.get("costUSD").is_none() && !unsupported {
        buckets.and_then(|b| estimate(model, b, fast, prices))
    } else {
        None
    };
    Ok(Some(Entry {
        event: Event {
            id: String::new(),
            model: model.into(),
            at,
            tokens,
            ticks,
        },
        message: message.into(),
        request: request.into(),
        side,
        speed: speed.is_some(),
        unsupported,
        estimate,
    }))
}
fn estimate(model: &str, b: [u64; 5], fast: bool, prices: &Prices) -> Option<u64> {
    let model = prices.aliases.get(model).map_or(model, String::as_str);
    let r = prices.rates.get(model)?;
    if fast && r.fast.is_none() && !model.ends_with("-fast") {
        return None;
    }
    let mut rates = [r.input, r.output, r.read, r.write, r.input * 2];
    if b[0] + b[2] + b[3] + b[4] > 200_000 {
        for (i, long) in [r.input_long, r.output_long, r.read_long, r.write_long]
            .into_iter()
            .enumerate()
        {
            if let Some(n) = long {
                rates[i] = n;
            }
        }
        rates[4] = rates[0] * 2;
    }
    let n = b
        .into_iter()
        .zip(rates)
        .try_fold(0_u64, |n, (b, r)| n.checked_add(b.checked_mul(r)?))?
        .checked_mul(if fast { r.fast.unwrap_or(1) } else { 1 })?;
    (n <= 1_000_000_000_000_000).then_some(n)
}
fn dedup(entries: Vec<Entry>) -> (Vec<Entry>, usize) {
    let mut result: Vec<Entry> = vec![];
    let mut exact = BTreeMap::new();
    let mut messages: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for e in entries {
        let key = (e.message.clone(), e.request.clone());
        let collision = exact.get(&key).copied().or_else(|| {
            messages
                .get(&e.message)
                .and_then(|a| a.iter().copied().find(|i| e.side || result[*i].side))
        });
        if let Some(i) = collision {
            let old = &result[i];
            if old.event.model != e.event.model {
                conflicts.insert(i);
                continue;
            }
            let replace = if e.side != old.side {
                old.side
            } else if e.event.tokens.unwrap_or(0) != old.event.tokens.unwrap_or(0) {
                e.event.tokens.unwrap_or(0) > old.event.tokens.unwrap_or(0)
            } else if e.event.ticks.is_some() != old.event.ticks.is_some() {
                e.event.ticks.is_some()
            } else {
                e.speed && !old.speed
            };
            if replace {
                exact.remove(&(old.message.clone(), old.request.clone()));
                result[i] = e;
                exact.insert(key, i);
            }
        } else {
            let i = result.len();
            exact.insert(key, i);
            messages.entry(e.message.clone()).or_default().push(i);
            result.push(e);
        }
    }
    let accepted = result
        .into_iter()
        .enumerate()
        .filter_map(|(i, mut e)| {
            if conflicts.contains(&i) {
                None
            } else {
                e.event.id = i.to_string();
                Some(e)
            }
        })
        .collect();
    (accepted, conflicts.len())
}

/// Read-only local-machine history. No credentials, persistent cache, or network pricing.
#[must_use]
pub fn scan_claude_history<T: TimeZone>(
    dir: &Path,
    now: &DateTime<T>,
    timezone: &str,
    cancel: &Cancellation,
) -> LocalHistory {
    let mut h = LocalHistory {
        scope: "local_machine".into(),
        attribution: "unattributed".into(),
        currency: "USD".into(),
        cost_basis: "unknown".into(),
        price_snapshot: None,
        timezone: timezone.into(),
        status: "no_data".into(),
        excluded_events: 0,
        daily: vec![],
        periods: vec![],
    };
    let Ok(prices) = serde_json::from_str::<Prices>(include_str!("claude_prices.json")) else {
        h.status = "unavailable".into();
        return h;
    };
    h.price_snapshot = Some(prices.snapshot.clone());
    if !dir.is_absolute()
        || dir.as_os_str().as_encoded_bytes().contains(&b',')
        || dir
            .as_os_str()
            .as_encoded_bytes()
            .split(|b| *b == b'/')
            .any(|c| c == b"." || c == b"..")
    {
        h.status = "unavailable".into();
        return h;
    }
    let mut entries = vec![];
    let read = crate::history_files::read_json(dir, "projects", true, cancel, |line| {
        let Some(e) = parse(line, &prices)? else {
            return Ok(vec![]);
        };
        let event = e.event.clone();
        entries.push(e);
        Ok(vec![event])
    });
    let Ok((_, excluded)) = read else {
        h.status = "unavailable".into();
        return h;
    };
    let (entries, conflicts) = dedup(entries);
    h.excluded_events = excluded + conflicts + entries.iter().filter(|e| e.unsupported).count();
    crate::history::aggregate(
        &mut h,
        entries.iter().map(|e| e.event.clone()).collect(),
        now,
    );
    apply_costs(&mut h, &entries, now);
    h
}

pub(crate) mod costs;
impl Entry {
    pub(crate) fn from_history(event: Event, estimate: Option<u64>) -> Self {
        Self {
            event,
            estimate,
            message: String::new(),
            request: String::new(),
            side: false,
            speed: false,
            unsupported: false,
        }
    }
}
use costs::apply_costs;

pub(crate) fn collect_enabled(
    report: &mut crate::Report,
    selected: bool,
    cancelled: &impl Fn() -> bool,
    report_deadline: Option<std::time::Instant>,
) {
    if selected && std::env::var("SYMBRAIN_USAGE_CLAUDE_HISTORY").as_deref() == Ok("1") {
        let cancel =
            report_deadline.map_or_else(Cancellation::default, Cancellation::with_deadline);
        if cancelled() {
            cancel.cancel();
        }
        let home = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|v| !v.is_empty())
            .map_or_else(
                || {
                    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".claude")
                },
                std::path::PathBuf::from,
            );
        for p in &mut report.providers {
            if p.id == "claude" {
                p.history = Some(scan_claude_history(
                    &home,
                    &chrono::Local::now(),
                    "local",
                    &cancel,
                ));
            }
        }
    }
}
