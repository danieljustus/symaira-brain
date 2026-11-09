//! Regression tests for #1012: a first-run backup must not hold the command
//! hostage, and a killed backup must not leave orphans behind.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{Backup, Outcome, Root, ensure, lock, verify_tree};

/// Files named `slow-*` take this long to copy, standing in for a state tree
/// that needs minutes to copy.
const SLOW_FILE: Duration = Duration::from_millis(100);
const CHILD_ENV: &str = "SYMAIRA_CUTOVER_TEST_CHILD_BASE";

/// Source files whose copy started, across all tests (filter by temp dir).
static COPIES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn copies_under(base: &Path) -> usize {
    let copies = COPIES.lock().unwrap();
    copies.iter().filter(|path| path.starts_with(base)).count()
}

pub(crate) fn slow_down(path: &Path) {
    COPIES.lock().unwrap().push(path.to_path_buf());
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("slow-"))
    {
        std::thread::sleep(SLOW_FILE);
    }
}

fn roots(base: &Path) -> Vec<Root> {
    vec![Root {
        label: "data",
        path: base.join("data/symbrain"),
    }]
}

/// A synthetic state tree: `count` slow files, so a full copy takes at least
/// `count * SLOW_FILE`.
fn seed_slow(base: &Path, count: usize) -> PathBuf {
    let data = base.join("data/symbrain/memory");
    fs::create_dir_all(&data).unwrap();
    for index in 0..count {
        fs::write(data.join(format!("slow-{index:04}.db")), vec![7_u8; 4096]).unwrap();
    }
    base.join("store")
}

fn entries(store: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(store)
        .map(|dir| {
            dir.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn partials(store: &Path) -> Vec<String> {
    entries(store)
        .into_iter()
        .filter(|name| name.ends_with(".partial"))
        .collect()
}

#[test]
fn slow_backup_does_not_hold_the_command_hostage() {
    let temp = tempfile::tempdir().unwrap();
    let count = 60;
    let store = seed_slow(temp.path(), count);
    let started = Instant::now();
    let mut backup = Backup::spawn("symbrain", roots(temp.path()), store.clone());
    assert!(backup.settle(Duration::from_millis(200)));
    assert!(
        started.elapsed() < SLOW_FILE * 30,
        "start + settle took {:?}, the full copy needs {:?}",
        started.elapsed(),
        SLOW_FILE * 60
    );
    assert!(
        !store.join("symbrain.json").exists(),
        "the copy is still running after the grace period"
    );
    // The command ran meanwhile; the backup then completes and verifies.
    backup.finish(Duration::from_secs(60));
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("symbrain.json")).unwrap()).unwrap();
    assert_eq!(marker["state"], "backed-up");
    assert_eq!(marker["files"], count);
    assert!(partials(&store).is_empty());
    verify_tree(Path::new(marker["backup"].as_str().unwrap())).unwrap();
}

#[test]
fn quick_backup_finishes_inside_the_start_grace() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed_slow(temp.path(), 2);
    let mut backup = Backup::spawn("symbrain", roots(temp.path()), store.clone());
    assert!(backup.settle(Duration::from_secs(60)));
    assert!(store.join("symbrain.json").exists());
}

#[test]
fn failed_backup_is_reported_by_settle() {
    let temp = tempfile::tempdir().unwrap();
    seed_slow(temp.path(), 1);
    // The store's parent is a regular file, so the store cannot be created.
    let blocker = temp.path().join("blocker");
    fs::write(&blocker, b"file").unwrap();
    let mut backup = Backup::spawn("symbrain", roots(temp.path()), blocker.join("store"));
    assert!(!backup.settle(Duration::from_secs(60)));
}

#[test]
fn cancelled_backup_is_resumed_without_recopying_finished_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed_slow(temp.path(), 100);
    let mut backup = Backup::spawn("symbrain", roots(temp.path()), store.clone());
    assert!(backup.settle(Duration::from_millis(450)));
    let started = Instant::now();
    backup.finish(Duration::from_millis(50));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!store.join("symbrain.json").exists());
    assert_eq!(partials(&store).len(), 1, "{:?}", entries(&store));
    let first = copies_under(temp.path());
    assert!((2..100).contains(&first), "first run copied {first}");

    // A source file removed meanwhile must not survive in the backup.
    let gone = temp.path().join("data/symbrain/memory/slow-0000.db");
    fs::remove_file(&gone).unwrap();
    let outcome = ensure("symbrain", &roots(temp.path()), &store).unwrap();
    let Outcome::Created { dir, files, .. } = outcome else {
        panic!("expected a backup, got {outcome:?}");
    };
    assert_eq!(files, 99);
    // At most the file the cancelled run was writing is copied twice.
    let second = copies_under(temp.path()) - first;
    assert!(
        second <= 99 - (first - 1) + 1,
        "{first} then {second} copies"
    );
    assert!(!dir.join("data/memory/slow-0000.db").exists());
    verify_tree(&dir).unwrap();
    assert!(partials(&store).is_empty(), "{:?}", entries(&store));
}

/// Body of the child process of [`killed_backup_leaves_no_orphan_for_the_next_run`].
#[test]
fn child_backup_process() {
    let Some(base) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
        return;
    };
    let _ = ensure("symbrain", &roots(&base), &base.join("store"));
}

#[test]
fn killed_backup_leaves_no_orphan_for_the_next_run() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed_slow(temp.path(), 100);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests_background::child_backup_process",
            "--nocapture",
        ])
        .env(CHILD_ENV, temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while partials(&store).is_empty() {
        assert!(Instant::now() < deadline, "child never started copying");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(partials(&store).len(), 1, "SIGKILL leaves the partial tree");
    assert!(!store.join("symbrain.json").exists());

    let outcome = ensure("symbrain", &roots(temp.path()), &store).unwrap();
    let Outcome::Created { dir, files, .. } = outcome else {
        panic!("expected a backup, got {outcome:?}");
    };
    assert_eq!(files, 100);
    verify_tree(&dir).unwrap();
    assert!(partials(&store).is_empty(), "{:?}", entries(&store));
}

#[test]
fn stale_partials_are_removed_and_other_apps_are_left_alone() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed_slow(temp.path(), 1);
    for name in [
        ".symbrain-20260101T000000Z-1.partial",
        ".symbrain-20260102T000000Z-2.partial",
        ".symguard-20260101T000000Z-3.partial",
        ".symbrain-extra-20260101T000000Z-4.partial",
    ] {
        fs::create_dir_all(store.join(name).join("data")).unwrap();
        fs::write(store.join(name).join("data/file"), b"x").unwrap();
    }
    fs::write(store.join("symbrain.tmp-99"), b"{}").unwrap();
    let outcome = ensure("symbrain", &roots(temp.path()), &store).unwrap();
    assert!(matches!(outcome, Outcome::Created { .. }));
    assert_eq!(
        partials(&store),
        [
            ".symbrain-extra-20260101T000000Z-4.partial",
            ".symguard-20260101T000000Z-3.partial"
        ]
    );
    assert!(!store.join("symbrain.tmp-99").exists());
}

#[test]
fn live_backup_is_not_disturbed_by_a_second_run() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed_slow(temp.path(), 1);
    fs::create_dir_all(&store).unwrap();
    let partial = store.join(".symbrain-20260101T000000Z-1.partial");
    fs::create_dir_all(&partial).unwrap();
    let slot = lock::try_acquire(&store, "symbrain").unwrap().unwrap();
    assert!(lock::try_acquire(&store, "symbrain").unwrap().is_none());
    let outcome = ensure("symbrain", &roots(temp.path()), &store).unwrap();
    assert_eq!(outcome, Outcome::InProgress);
    assert!(
        partial.exists(),
        "a live run's partial tree is never removed"
    );
    assert!(!store.join("symbrain.json").exists());
    drop(slot);
    let outcome = ensure("symbrain", &roots(temp.path()), &store).unwrap();
    assert!(matches!(outcome, Outcome::Created { .. }));
    assert!(!partial.exists());
}
