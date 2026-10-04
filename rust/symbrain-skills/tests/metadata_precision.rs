//! Lifecycle events and the observed usage gap retain sub-second ordering.
use std::collections::BTreeMap;
use std::fs;
use symbrain_skills::install::OperationEvent;
use symbrain_skills::metadata::{Options, collect};
use tempfile::tempdir;

fn event(timestamp: &str, path: &std::path::Path) -> OperationEvent {
    OperationEvent {
        ts: timestamp.into(),
        event: "install".into(),
        skill: "demo".into(),
        target: "opencode".into(),
        path: path.to_string_lossy().into_owned(),
        outcome: "ok".into(),
        ..Default::default()
    }
}

#[test]
fn later_nanoseconds_win_even_when_events_are_read_in_reverse_order() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("SKILL.md"), "body").unwrap();
    let latest = "2020-01-02T03:04:05.999999999Z";
    let options = Options {
        events: Some(BTreeMap::from([(
            "demo".into(),
            vec![
                event(latest, root.path()),
                event("2020-01-02T03:04:05.000000001Z", root.path()),
            ],
        )])),
        ..Default::default()
    };
    let record = collect(root.path(), "demo", &options);
    assert_eq!(record.installs.len(), 1);
    assert_eq!(record.installs[0].installed_at, latest);
}

#[cfg(unix)]
#[test]
fn last_used_requires_the_full_nanosecond_minute_gap() {
    use std::time::{Duration, UNIX_EPOCH};
    let root = tempdir().unwrap();
    let path = root.path().join("SKILL.md");
    fs::write(&path, "body").unwrap();
    let file = fs::OpenOptions::new().read(true).open(path).unwrap();
    let options = Options {
        events: Some(BTreeMap::from([(
            "demo".into(),
            vec![event("2020-01-02T03:04:05.999999999Z", root.path())],
        )])),
        ..Default::default()
    };
    let seconds = 1_577_934_245;
    let written = UNIX_EPOCH + Duration::from_secs(seconds);
    let before = written + Duration::new(60, 999_999_998);
    file.set_times(
        fs::FileTimes::new()
            .set_modified(written)
            .set_accessed(before),
    )
    .unwrap();
    assert!(collect(root.path(), "demo", &options).last_used.is_none());
    let boundary = written + Duration::new(60, 999_999_999);
    file.set_times(
        fs::FileTimes::new()
            .set_modified(written)
            .set_accessed(boundary),
    )
    .unwrap();
    let record = collect(root.path(), "demo", &options);
    assert_eq!(
        record.last_used.as_deref(),
        Some("2020-01-02T03:05:05.999999999Z")
    );
    assert_eq!(record.last_used_source, "install_atime");
}
