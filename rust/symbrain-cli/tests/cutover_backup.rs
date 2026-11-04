//! First Rust run over Go-created state: automatic verified backup, unchanged
//! reads, one-time behavior and the documented `cp -Rp` restore.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_symbrain");
/// Memory store written by the Go release (see fixtures/memory_native/README.md).
const GO_SEED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/memory_native/memory_serve_oracle/seed.db"
);

fn seed_home(home: &Path) {
    let config = home.join(".config/symbrain");
    fs::create_dir_all(config.join("profiles")).unwrap();
    fs::write(
        config.join("profiles/work.toml"),
        "name = \"work\"\ndescription = \"Go-era profile\"\n",
    )
    .unwrap();
    let memory = home.join(".local/share/symbrain/memory");
    fs::create_dir_all(&memory).unwrap();
    fs::copy(GO_SEED, memory.join("default.db")).unwrap();
}

fn run(home: &Path, backup: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("SYMAIRA_CUTOVER_BACKUP", backup)
        .args(args)
        .output()
        .unwrap()
}

fn backups(home: &Path) -> Vec<PathBuf> {
    let store = home.join(".local/state/symaira/cutover-backups");
    let mut dirs: Vec<_> = fs::read_dir(store)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

const LIST: &[&str] = &["memory", "list", "--json"];

#[test]
fn first_rust_run_backs_up_go_state_once_and_reads_it_unchanged() {
    let reference = tempfile::tempdir().unwrap();
    seed_home(reference.path());
    let expected = run(reference.path(), "off", LIST);
    assert!(expected.status.success(), "{expected:?}");
    assert!(backups(reference.path()).is_empty());

    let temp = tempfile::tempdir().unwrap();
    seed_home(temp.path());
    let first = run(temp.path(), "on", LIST);
    assert!(first.status.success(), "{first:?}");
    assert_eq!(first.stdout, expected.stdout, "backup must not change the read");
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        stderr.contains("symbrain: backed up existing state (2 files) to "),
        "{stderr}"
    );
    let dirs = backups(temp.path());
    assert_eq!(dirs.len(), 1);
    symaira_cutover::verify_tree(&dirs[0]).unwrap();
    assert_eq!(
        fs::read(dirs[0].join("data/memory/default.db")).unwrap(),
        fs::read(GO_SEED).unwrap(),
        "backup holds the Go-written bytes"
    );

    let second = run(temp.path(), "on", LIST);
    assert!(second.status.success());
    assert_eq!(second.stdout, expected.stdout);
    assert!(second.stderr.is_empty(), "{:?}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(backups(temp.path()).len(), 1, "the backup runs once");
}

#[test]
fn documented_restore_recovers_the_go_state() {
    let temp = tempfile::tempdir().unwrap();
    seed_home(temp.path());
    let before = run(temp.path(), "on", LIST);
    assert!(before.status.success());
    let backup = backups(temp.path()).pop().unwrap();

    // A broken first run: the memory store and a profile are lost.
    fs::remove_dir_all(temp.path().join(".local/share/symbrain")).unwrap();
    fs::remove_file(temp.path().join(".config/symbrain/profiles/work.toml")).unwrap();
    assert!(symaira_cutover::verify_restore(&backup).is_err());

    for (label, original) in [("config", ".config/symbrain"), ("data", ".local/share/symbrain")] {
        let original = temp.path().join(original);
        fs::create_dir_all(&original).unwrap();
        let status = Command::new("cp")
            .arg("-Rp")
            .arg(backup.join(label).join("."))
            .arg(&original)
            .status()
            .unwrap();
        assert!(status.success());
    }
    symaira_cutover::verify_restore(&backup).unwrap();
    let after = run(temp.path(), "on", LIST);
    assert!(after.status.success());
    assert_eq!(after.stdout, before.stdout, "restored state reads identically");
}

#[test]
fn failed_backup_stops_before_the_command_touches_state() {
    let temp = tempfile::tempdir().unwrap();
    seed_home(temp.path());
    // The store's parent is a regular file, so the backup cannot be created.
    fs::create_dir_all(temp.path().join(".local/state")).unwrap();
    fs::write(temp.path().join(".local/state/symaira"), b"not a directory").unwrap();
    let db = temp.path().join(".local/share/symbrain/memory/default.db");
    let before = fs::read(&db).unwrap();
    let output = run(
        temp.path(),
        "on",
        &["memory", "set", "--content", "must not be written"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("automatic state backup before the first Rust run failed"),
        "{stderr}"
    );
    assert_eq!(fs::read(&db).unwrap(), before, "state untouched");
}
