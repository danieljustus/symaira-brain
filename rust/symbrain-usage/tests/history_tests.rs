#![cfg(unix)]
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use symbrain_usage::{Cancellation, Report, scan_grok_history};

#[derive(Deserialize)]
struct Corpus {
    now: String,
    timezone: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    files: BTreeMap<String, String>,
    status: String,
    excluded: usize,
}
fn cases() -> Corpus {
    serde_json::from_str(include_str!("fixtures/grok-history-cases.json")).unwrap()
}
fn fixture(files: &BTreeMap<String, String>) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for (path, data) in files {
        let path = home.path().join("sessions").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }
    home
}
#[test]
fn grok_history_paired_cases() {
    let cases = cases();
    let now: DateTime<FixedOffset> = cases.now.parse().unwrap();
    for c in cases.cases {
        let home = fixture(&c.files);
        let h = scan_grok_history(home.path(), &now, &cases.timezone, &Cancellation::default());
        assert_eq!(h.status, c.status, "{}", c.name);
        assert_eq!(h.excluded_events, c.excluded, "{}", c.name);
        assert_eq!(h.attribution, "unattributed");
        assert_eq!(h.scope, "local_machine");
        let encoded = serde_json::to_string(&h).unwrap();
        println!("HISTORY_CASE {} {encoded}", c.name);
        assert!(!encoded.contains("eventId") && !encoded.contains("inputTokens"));
        match c.name.as_str() {
            "replays-subagents-forks-resume" => {
                let m = &h.periods[0].models[0];
                assert_eq!(m.events, 3);
                assert_eq!(m.tokens.as_deref(), Some("470"));
                assert_eq!(m.recorded_cost.as_deref(), Some("1.47143176"));
            }
            "true-zero" => {
                let m = &h.daily[0].models[0];
                assert_eq!(m.tokens.as_deref(), Some("0"));
                assert_eq!(m.recorded_cost.as_deref(), Some("0"));
            }
            "missing-measurements" => {
                let m = &h.daily[0].models[0];
                assert!(m.tokens.is_none() && m.recorded_cost.is_none());
                assert_eq!((m.missing_tokens, m.missing_cost), (1, 1));
            }
            "calendar-boundaries" => {
                assert_eq!(h.daily.len(), 5);
                assert_eq!(h.daily[0].key, "2026-09-29");
                assert_eq!(h.periods[0].models[0].events, 5);
            }
            _ => (),
        }
    }
}
#[test]
fn history_legacy_contract() {
    let data = r#"{"schema_version":1,"providers":[{"id":"grok","display_name":"Grok","configured":false,"auth_status":{"status":"missing","detail":"none"}}]}"#;
    let mut r: Report = serde_json::from_str(data).unwrap();
    assert_eq!(serde_json::to_string(&r).unwrap(), data);
    let home = tempfile::tempdir().unwrap();
    let now = chrono::Utc::now();
    r.providers[0].history = Some(scan_grok_history(
        home.path(),
        &now,
        "UTC",
        &Cancellation::default(),
    ));
    let encoded = serde_json::to_string(&r).unwrap();
    let decoded: Report = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.providers[0].history.as_ref().unwrap().status,
        "no_data"
    );
}
#[test]
fn grok_history_path_safety_and_budgets() {
    for kind in [
        "file-link",
        "dir-link",
        "home-link",
        "fifo",
        "oversize",
        "depth",
        "files",
        "cancel",
        "dotdot",
        "relative",
        "entries",
        "events",
        "line",
    ] {
        safety_case(kind);
    }
}
fn safety_case(kind: &str) {
    use std::os::unix::fs::symlink;
    let cases = cases();
    let home = fixture(&cases.cases[1].files);
    let path = home.path().join("sessions/p/updates.jsonl");
    let alias = tempfile::tempdir().unwrap();
    let mut root = home.path().to_path_buf();
    let cancel = Cancellation::default();
    match kind {
        "file-link" => {
            std::fs::remove_file(&path).unwrap();
            symlink(home.path().join("outside"), &path).unwrap();
        }
        "dir-link" => symlink(home.path(), home.path().join("sessions/alias")).unwrap(),
        "home-link" => {
            root = alias.path().join("alias");
            symlink(home.path(), &root).unwrap();
        }
        "fifo" => {
            std::fs::remove_file(&path).unwrap();
            assert!(
                std::process::Command::new("/usr/bin/mkfifo")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        "oversize" => std::fs::write(&path, vec![0; (4 << 20) + 1]).unwrap(),
        "depth" => {
            std::fs::create_dir_all(home.path().join("sessions").join("d/".repeat(14))).unwrap();
        }
        "files" => {
            for i in 0..129 {
                let dir = home.path().join(format!("sessions/{i}"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("updates.jsonl"), []).unwrap();
            }
        }
        "cancel" => cancel.cancel(),
        "dotdot" => root = root.join("..").join(home.path().file_name().unwrap()),
        "relative" => root = Path::new("relative").into(),
        "entries" => {
            for i in 0..4097 {
                std::fs::write(home.path().join(format!("sessions/{i}")), []).unwrap();
            }
        }
        "events" => {
            std::fs::write(&path, cases.cases[1].files["p/updates.jsonl"].repeat(10001)).unwrap();
        }
        "line" => std::fs::write(&path, format!("{}1\n", " ".repeat(256 << 10))).unwrap(),
        _ => unreachable!(),
    }
    let started = std::time::Instant::now();
    let h = scan_grok_history(&root, &chrono::Utc::now(), "UTC", &cancel);
    assert_eq!(h.status, "unavailable", "{kind}");
    assert!(h.daily.is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}
