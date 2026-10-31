use crate::{claude_history::Entry, history::Event};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

fn record(line: &[u8]) -> Result<Value, ()> {
    let o: Value = serde_json::from_slice(line).map_err(|_| ())?;
    o.as_object().ok_or(())?;
    for key in ["type", "timestamp"] {
        if o.get(key).is_some_and(|v| !v.is_null() && !v.is_string()) {
            return Err(());
        }
    }
    if o.get("payload")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(());
    }
    Ok(o)
}

// Cached input and reasoning output are subsets of input and output respectively.
fn usage(v: &Value) -> Result<[u64; 4], ()> {
    let u = v.as_object().ok_or(())?;
    let mut b = [0; 4];
    for (i, keys) in [
        &["input_tokens", "prompt_tokens", "input"][..],
        &[
            "cached_input_tokens",
            "cache_read_input_tokens",
            "cached_tokens",
        ][..],
        &["output_tokens", "completion_tokens", "output"][..],
        &["reasoning_output_tokens", "reasoning_tokens"][..],
    ]
    .iter()
    .enumerate()
    {
        if let Some(v) = keys.iter().find_map(|k| u.get(*k)) {
            b[i] = v.as_u64().filter(|n| *n <= 1_000_000_000_000).ok_or(())?;
        } else if i == 0 || i == 2 {
            return Err(());
        }
    }
    if b[1] > b[0] || b[3] > b[2] {
        return Err(());
    }
    if let Some(total) = u.get("total_tokens")
        && total.as_u64() != Some(b[0] + b[2])
    {
        return Err(());
    }
    Ok(b)
}
fn model(v: &Value) -> Option<&str> {
    [&v["model"], &v["model_name"], &v["metadata"]["model"]]
        .into_iter()
        .find_map(|v| v.as_str().filter(|s| !s.is_empty()))
}
fn non_null(v: &Value) -> bool {
    !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty())
}
fn valid_model(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.contains("..")
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
#[derive(Default)]
pub(super) struct Parser {
    previous: Option<[u64; 4]>,
    model: String,
    tier: String,
    session: String,
    meta: bool,
    child: bool,
    created: Option<DateTime<Utc>>,
    poisoned: bool,
    last_only_seen: BTreeMap<String, [u64; 4]>,
    replay_seen: BTreeSet<String>,
}
impl Parser {
    pub(super) fn parse(&mut self, line: &[u8]) -> Result<Option<Entry>, ()> {
        let result = self.line(line);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn update_context(&mut self, o: &Value) -> bool {
        let v = &o["payload"];
        let kind = v["type"].as_str().unwrap_or_default();
        if o["type"] == "session_meta" && !self.meta {
            self.meta = true;
            self.session = v["id"].as_str().unwrap_or_default().into();
            self.child = non_null(&v["forked_from_id"])
                || non_null(&v["parent_thread_id"])
                || v["thread_source"] == "subagent"
                || non_null(&v["source"]["subagent"]);
            self.created = o["timestamp"].as_str().and_then(|s| s.parse().ok());
            return true;
        }
        if o["type"] == "turn_context" {
            if let Some(m) = model(v) {
                self.model = m.into();
            }
            return true;
        }
        if o["type"] != "event_msg" {
            return true;
        }
        if kind == "thread_settings_applied" {
            if let Some(tier) = v["thread_settings"]
                .get("service_tier")
                .or_else(|| v.get("service_tier"))
            {
                self.tier = tier.as_str().unwrap_or("unsupported").into();
            }
            return true;
        }
        if kind == "task_started" && self.child {
            let gate = self.created.or_else(|| {
                o["timestamp"]
                    .as_str()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            });
            if let Some((start, gate)) = v["started_at"].as_f64().zip(gate)
                && start
                    >= serde_json::Number::from(gate.timestamp())
                        .as_f64()
                        .unwrap_or(f64::INFINITY)
            {
                self.child = false;
            }
            return true;
        }
        if kind != "token_count" {
            return true;
        }
        false
    }
    // Pricing metadata must not advance copied last-only replay twice.
    fn last_snapshot(&mut self, id: &str, b: [u64; 4]) -> Option<[u64; 4]> {
        let next = *self.last_only_seen.entry(id.to_owned()).or_insert_with(|| {
            let mut next = b;
            if let Some(previous) = self.previous {
                for (n, p) in next.iter_mut().zip(previous) {
                    *n += p;
                }
            }
            self.previous = Some(next);
            next
        });
        if self.child {
            self.replay_seen.insert(id.to_owned());
        }
        // A copied replay prefix is not made live by a task-start marker.
        (!self.replay_seen.contains(id)).then_some(next)
    }
    fn line(&mut self, line: &[u8]) -> Result<Option<Entry>, ()> {
        let o = record(line)?;
        if self.update_context(&o) {
            return Ok(None);
        }
        let v = &o["payload"];
        let Some(info) = v.get("info").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        info.as_object().ok_or(())?;
        let totals = info
            .get("total_token_usage")
            .filter(|v| !v.is_null())
            .map(usage)
            .transpose()?;
        if totals.is_some() && totals == self.previous {
            return Ok(None);
        }
        let used = if let Some(last) = info.get("last_token_usage").filter(|v| !v.is_null()) {
            Some(usage(last)?)
        } else {
            totals.map(|mut b| {
                if let Some(previous) = self.previous
                    && b.iter().zip(previous).all(|(a, b)| *a >= b)
                {
                    for (a, b) in b.iter_mut().zip(previous) {
                        *a -= b;
                    }
                }
                b
            })
        };
        if totals.is_some() {
            self.previous = totals;
        }
        let Some(b) = used else {
            return Ok(None);
        };
        if b[0] + b[2] == 0 {
            return Ok(None);
        }
        let at = o["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .ok_or(())?;
        if let Some(m) = model(v).or_else(|| model(info)) {
            self.model = m.into();
        }
        let m = if self.model.is_empty() {
            "unknown".to_owned()
        } else {
            self.model.clone()
        };
        if !valid_model(&m) || b[1] > b[0] || b[3] > b[2] {
            return Err(());
        }
        let baseline_id = format!("{}|{at}|{m}|{b:?}", self.session);
        let mut snapshot = totals;
        if totals.is_none() && !self.poisoned {
            let Some(next) = self.last_snapshot(&baseline_id, b) else {
                return Ok(None);
            };
            snapshot = Some(next);
        }
        if self.child {
            return Ok(None);
        }
        let id = if self.poisoned {
            String::new()
        } else {
            let snapshot = snapshot.unwrap();
            // Hex keeps the final separator unambiguous during copy reconciliation.
            let tier = self.tier.as_bytes().iter().fold(String::new(), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            });
            format!("{baseline_id}|{snapshot:?}|{tier}")
        };
        Ok(Some(Entry::from_history(Event {
            id,
            model: m,
            at,
            tokens: (!self.poisoned).then_some(b[0] + b[2]),
        })))
    }
}
