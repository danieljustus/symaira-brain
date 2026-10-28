//! Canonical rollout-only history; offline estimates, no account attribution or credential I/O.
use crate::{Cancellation, LocalHistory, claude_history::Entry};
use chrono::{DateTime, TimeZone};
mod pricing;
use pricing::Prices;
mod parser;
use parser::Parser;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn merge_duplicate(old: &mut Entry, e: Entry) -> bool {
    let mut conflict = false;
    if old.event.ticks.is_none() && e.event.ticks.is_some() {
        *old = e;
    } else if old.event.ticks.is_none() {
        if old.estimate.is_some() && e.estimate.is_some() && old.estimate != e.estimate {
            old.estimate = None;
            conflict = true;
        }
        if e.estimate.is_none() {
            old.estimate = None;
        }
    }
    conflict
}

/// One absolute native home, sessions and archives only. No credential or source writes.
#[must_use]
pub fn scan_codex_history<T: TimeZone>(
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
    let Ok(prices) = serde_json::from_str::<Prices>(include_str!("codex_prices.json")) else {
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
            .any(|p| p == b"." || p == b"..")
    {
        h.status = "unavailable".into();
        return h;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut entries: Vec<Entry> = vec![];
    let mut seen = BTreeMap::<String, usize>::new();
    let mut conflicts = 0;
    let mut files = BTreeSet::new();
    for sub in ["sessions", "archived_sessions"] {
        let mut parser = Parser::default();
        let read = crate::history_files::read_json_reset(
            dir,
            sub,
            true,
            cancel,
            Some(deadline),
            |line, relative| {
                if std::time::Instant::now() >= deadline {
                    return Err(());
                }
                let Some(line) = line else {
                    parser = Parser::default();
                    return Ok(files.insert(relative.to_owned()).then_some(vec![]));
                };
                let Some(mut e) = parser.parse(line, &prices)? else {
                    return Ok(Some(vec![]));
                };
                let measurement_id = if e.event.id.is_empty() {
                    e.event.id = format!("invalid-{}", entries.len());
                    e.event.id.clone()
                } else {
                    // Conflicting tiers on a copied measurement retain incomplete cost.
                    e.event.id.rsplit_once('|').ok_or(())?.0.to_owned()
                };
                if let Some(i) = seen.get(&measurement_id) {
                    conflicts += usize::from(merge_duplicate(&mut entries[*i], e));
                    return Ok(Some(vec![]));
                }
                seen.insert(measurement_id, entries.len());
                let event = e.event.clone();
                entries.push(e);
                Ok(Some(vec![event]))
            },
        );
        let Ok((_, excluded)) = read else {
            h.status = "unavailable".into();
            return h;
        };
        if entries.len() > 10000 {
            h.status = "unavailable".into();
            return h;
        }
        h.excluded_events += excluded;
        if std::time::Instant::now() >= deadline {
            h.status = "unavailable".into();
            return h;
        }
    }
    h.excluded_events += conflicts;
    crate::history::aggregate(
        &mut h,
        entries.iter().map(|e| e.event.clone()).collect(),
        now,
    );
    crate::claude_history::costs::apply_costs(&mut h, &entries, now);
    h
}
pub(crate) fn collect_enabled(
    report: &mut crate::Report,
    selected: bool,
    cancelled: &impl Fn() -> bool,
    deadline: Option<std::time::Instant>,
) {
    if !selected || std::env::var("SYMBRAIN_USAGE_CODEX_HISTORY").as_deref() != Ok("1") {
        return;
    }
    let cancel = deadline.map_or_else(Cancellation::default, Cancellation::with_deadline);
    if cancelled() {
        cancel.cancel();
    }
    let home = std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".codex")
            },
            std::path::PathBuf::from,
        );
    for p in &mut report.providers {
        if p.id == "codex" {
            p.history = Some(scan_codex_history(
                &home,
                &chrono::Local::now(),
                "local",
                &cancel,
            ));
        }
    }
}
