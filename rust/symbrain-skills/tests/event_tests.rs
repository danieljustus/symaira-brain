use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fs2::FileExt;
use symbrain_skills::install::{
    EVENT_MAX_BYTES, InstallOptions, OperationEvent, install_copy, record_event_at, uninstall,
};

const FIXED_TIMESTAMP: &str = "2026-09-07T12:34:56Z";

fn event(error: String) -> OperationEvent {
    OperationEvent {
        ts: String::new(),
        event: "test".to_owned(),
        skill: "boundary".to_owned(),
        target: "opencode".to_owned(),
        skill_version: "1.0.0".to_owned(),
        source_hash: "source".to_owned(),
        scope: "user".to_owned(),
        mode: "copy".to_owned(),
        path: "/tmp/skill".to_owned(),
        outcome: "ok".to_owned(),
        error,
        actor: "test".to_owned(),
        tool_version: "test-tool".to_owned(),
    }
}

fn rotated_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.1.{}",
        path.file_stem().expect("event file stem").to_string_lossy(),
        path.extension().expect("event extension").to_string_lossy()
    ))
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.lock",
        path.file_name().expect("event file name").to_string_lossy()
    ))
}

#[test]
fn exact_mib_boundary_stays_current_and_next_record_rotates() {
    let dir = tempfile::tempdir().expect("event directory");
    let path = dir.path().join("events.jsonl");
    let mut base_event = event(String::new());
    base_event.ts = FIXED_TIMESTAMP.to_owned();
    let base_len = serde_json::to_vec(&base_event).expect("base event").len() + 1;
    let mut one_byte_event = event("x".to_owned());
    one_byte_event.ts = FIXED_TIMESTAMP.to_owned();
    let error_overhead = serde_json::to_vec(&one_byte_event)
        .expect("one-byte error event")
        .len()
        - serde_json::to_vec(&base_event).expect("base event").len();
    let error_len = usize::try_from(EVENT_MAX_BYTES).expect("event size fits usize")
        - base_len
        - error_overhead
        + 1;
    let mut boundary = event("x".repeat(error_len));
    boundary.ts = FIXED_TIMESTAMP.to_owned();
    assert_eq!(
        serde_json::to_vec(&boundary).expect("boundary event").len() as u64 + 1,
        EVENT_MAX_BYTES
    );
    record_event_at(Some(&path), boundary, FIXED_TIMESTAMP);
    assert_eq!(
        fs::metadata(&path).expect("current segment").len(),
        EVENT_MAX_BYTES
    );
    assert!(!rotated_path(&path).exists());

    record_event_at(Some(&path), event(String::new()), FIXED_TIMESTAMP);
    assert_eq!(
        fs::metadata(rotated_path(&path))
            .expect("rotated segment")
            .len(),
        EVENT_MAX_BYTES
    );
    assert!(fs::metadata(&path).expect("new current segment").len() > 0);
}

#[test]
fn timestamp_and_tool_version_are_written_deterministically() {
    let dir = tempfile::tempdir().expect("event directory");
    let path = dir.path().join("events.jsonl");
    record_event_at(Some(&path), event(String::new()), FIXED_TIMESTAMP);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("event bytes")).expect("event JSON");
    assert_eq!(value["ts"], FIXED_TIMESTAMP);
    assert_eq!(value["tool_version"], "test-tool");
}

#[test]
fn real_child_processes_contend_without_corrupting_jsonl() {
    let dir = tempfile::tempdir().expect("event directory");
    let path = dir.path().join("events.jsonl");
    let held_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path(&path))
        .expect("lock file");
    held_lock.lock_exclusive().expect("hold event lock");

    let started = Instant::now();
    let mut blocked = child_writer(&path, "blocked");
    assert!(blocked.wait().expect("blocked child").success());
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(!path.exists(), "timed-out writer must not append");
    drop(held_lock);

    let mut children = (0..12)
        .map(|index| child_writer(&path, &index.to_string()))
        .collect::<Vec<_>>();
    for child in &mut children {
        assert!(child.wait().expect("writer child").success());
    }
    let lines = fs::read(&path)
        .expect("event log")
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).expect("valid child JSON"))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 12);
    assert!(
        lines
            .iter()
            .all(|line| line["tool_version"] == "child-tool")
    );
}

fn child_writer(path: &Path, index: &str) -> std::process::Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "event_child_writer", "--test-threads=1"])
        .env("SYMBRAIN_EVENT_CHILD_PATH", path)
        .env("SYMBRAIN_EVENT_CHILD_INDEX", index)
        .spawn()
        .expect("spawn event child")
}

#[test]
fn event_child_writer() {
    let Ok(path) = std::env::var("SYMBRAIN_EVENT_CHILD_PATH") else {
        return;
    };
    let index = std::env::var("SYMBRAIN_EVENT_CHILD_INDEX").expect("child index");
    let mut record = event(String::new());
    record.skill = format!("child-{index}");
    record.tool_version = "child-tool".to_owned();
    record_event_at(Some(Path::new(&path)), record, FIXED_TIMESTAMP);
}

fn source(root: &Path) {
    fs::create_dir_all(root).expect("source root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbody\n",
    )
    .expect("source skill");
}

#[test]
fn install_and_uninstall_success_and_refusal_events_are_recorded() {
    let dir = tempfile::tempdir().expect("event directory");
    let events = dir.path().join("events.jsonl");
    let source_dir = dir.path().join("source");
    source(&source_dir);
    let home_success = dir.path().join("success-home");
    let success_options = InstallOptions {
        home_dir: home_success.clone(),
        events_path: Some(events.clone()),
        ..Default::default()
    };
    install_copy(&source_dir, "demo", "hash", &success_options).expect("successful install");
    uninstall("opencode", "demo", &success_options).expect("successful uninstall");

    let home_install_refusal = dir.path().join("install-refusal-home");
    let install_destination = home_install_refusal.join(".config/opencode/skills/demo");
    fs::create_dir_all(&install_destination).expect("unmanaged install destination");
    fs::write(install_destination.join("foreign.txt"), b"foreign").expect("foreign file");
    let refusal_options = InstallOptions {
        home_dir: home_install_refusal,
        events_path: Some(events.clone()),
        ..Default::default()
    };
    assert!(install_copy(&source_dir, "demo", "hash", &refusal_options).is_err());

    let home_uninstall_refusal = dir.path().join("uninstall-refusal-home");
    let uninstall_destination = home_uninstall_refusal.join(".config/opencode/skills/demo");
    fs::create_dir_all(&uninstall_destination).expect("unmanaged uninstall destination");
    fs::write(uninstall_destination.join("foreign.txt"), b"foreign").expect("foreign file");
    let uninstall_refusal_options = InstallOptions {
        home_dir: home_uninstall_refusal,
        events_path: Some(events.clone()),
        ..Default::default()
    };
    assert!(uninstall("opencode", "demo", &uninstall_refusal_options).is_err());

    let records = fs::read_to_string(events)
        .expect("events")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("event record"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 4);
    assert_eq!(records[0]["event"], "install");
    assert_eq!(records[0]["outcome"], "ok");
    assert_eq!(records[1]["event"], "uninstall");
    assert_eq!(records[1]["outcome"], "ok");
    assert_eq!(records[2]["event"], "install");
    assert_eq!(records[2]["outcome"], "error");
    assert_eq!(records[3]["event"], "uninstall");
    assert_eq!(records[3]["outcome"], "error");
    for record in records {
        assert!(!record["ts"].as_str().unwrap_or_default().is_empty());
        assert!(
            !record["tool_version"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        );
    }
}
