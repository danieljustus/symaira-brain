//! Canonical rollout-only history; token counts, no account attribution or credential I/O.
use crate::{Cancellation, LocalHistory, claude_history::Entry};
use chrono::{DateTime, TimeZone};
mod parser;
use parser::Parser;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

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

        timezone: timezone.into(),
        status: "no_data".into(),
        excluded_events: 0,
        daily: vec![],
        periods: vec![],
    };
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
                let Some(mut e) = parser.parse(line)? else {
                    return Ok(Some(vec![]));
                };
                let measurement_id = if e.event.id.is_empty() {
                    e.event.id = format!("invalid-{}", entries.len());
                    e.event.id.clone()
                } else {
                    e.event.id.rsplit_once('|').ok_or(())?.0.to_owned()
                };
                if seen.contains_key(&measurement_id) {
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
    crate::history::aggregate(
        &mut h,
        entries.iter().map(|e| e.event.clone()).collect(),
        now,
    );
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
