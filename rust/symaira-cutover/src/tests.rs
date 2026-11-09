use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Outcome, Root, ensure, verify_restore, verify_tree};

fn roots(base: &Path) -> Vec<Root> {
    vec![
        Root {
            label: "config",
            path: base.join("config/symbrain"),
        },
        Root {
            label: "data",
            path: base.join("data/symbrain"),
        },
        Root {
            label: "legacy",
            path: base.join("data/symmemory"),
        },
    ]
}

/// Go-era layout: nested dirs, modes, an empty dir, a symlink, a non-UTF-8
/// name and a SQLite-like binary file.
fn seed(base: &Path) -> PathBuf {
    let config = base.join("config/symbrain");
    fs::create_dir_all(config.join("profiles/empty")).unwrap();
    fs::write(
        config.join("config.toml"),
        b"[vault]\nmode = \"request_only\"\n",
    )
    .unwrap();
    fs::write(config.join("profiles/work.toml"), b"name = \"work\"\n").unwrap();
    fs::set_permissions(
        config.join("config.toml"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    symlink("work.toml", config.join("profiles/default.toml")).unwrap();
    let data = base.join("data/symbrain/memory");
    fs::create_dir_all(&data).unwrap();
    fs::write(
        data.join("default.db"),
        (0..=255_u8).cycle().take(70_000).collect::<Vec<_>>(),
    )
    .unwrap();
    // Linux file systems accept non-UTF-8 names; APFS rejects them (EILSEQ).
    let odd = if cfg!(target_os = "linux") {
        std::ffi::OsStr::from_bytes(b"audit-\xff.jsonl")
    } else {
        std::ffi::OsStr::from_bytes("audit-\u{e9}.jsonl".as_bytes())
    };
    fs::write(base.join("data/symbrain").join(odd), b"{\"a\":1}\n").unwrap();
    fs::set_permissions(
        base.join("data/symbrain"),
        fs::Permissions::from_mode(0o750),
    )
    .unwrap();
    base.join("store")
}

/// Retries while another process briefly holds a duplicate of the slot's
/// descriptor: a sibling test's fork keeps the inherited `flock` until its
/// exec closes it, which on a loaded runner outlasts a single attempt.
fn ensure_when_free(roots: &[Root], store: &Path) -> Outcome {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let outcome = ensure("symbrain", roots, store).unwrap();
        if outcome != Outcome::InProgress || std::time::Instant::now() >= deadline {
            return outcome;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn created(outcome: Outcome) -> (PathBuf, u64, u64) {
    match outcome {
        Outcome::Created { dir, files, bytes } => (dir, files, bytes),
        other => panic!("expected a backup, got {other:?}"),
    }
}

#[test]
fn no_state_writes_marker_once_and_never_backs_up_later() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("store");
    assert_eq!(
        ensure("symbrain", &roots(temp.path()), &store).unwrap(),
        Outcome::NoState
    );
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("symbrain.json")).unwrap()).unwrap();
    assert_eq!(marker["state"], "none");
    seed(temp.path());
    assert_eq!(
        ensure("symbrain", &roots(temp.path()), &store).unwrap(),
        Outcome::AlreadyDone
    );
    assert_eq!(
        fs::read_dir(&store).unwrap().count(),
        1,
        "only the marker exists"
    );
}

#[test]
fn backup_copies_every_root_verifies_and_runs_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed(temp.path());
    let (dir, files, bytes) = created(ensure("symbrain", &roots(temp.path()), &store).unwrap());
    assert_eq!(files, 4);
    assert_eq!(bytes, 30 + 14 + 70_000 + 8);
    verify_tree(&dir).unwrap();
    assert_eq!(
        fs::read(dir.join("data/memory/default.db")).unwrap(),
        fs::read(temp.path().join("data/symbrain/memory/default.db")).unwrap()
    );
    assert_eq!(
        fs::read_link(dir.join("config/profiles/default.toml")).unwrap(),
        Path::new("work.toml")
    );
    assert!(dir.join("config/profiles/empty").is_dir());
    assert!(
        !dir.join("legacy").exists(),
        "absent roots are not invented"
    );
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&dir.join("config/config.toml")), 0o600);
    assert_eq!(mode(&dir.join("data")), 0o750);
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("symbrain.json")).unwrap()).unwrap();
    assert_eq!(marker["state"], "backed-up");
    assert_eq!(marker["backup"], dir.to_string_lossy().as_ref());
    assert_eq!(
        ensure("symbrain", &roots(temp.path()), &store).unwrap(),
        Outcome::AlreadyDone
    );
    let backups = fs::read_dir(&store)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().is_dir())
        .count();
    assert_eq!(backups, 1, "second run must not create another backup");
}

#[test]
fn tampered_backup_fails_verification() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed(temp.path());
    let (dir, ..) = created(ensure("symbrain", &roots(temp.path()), &store).unwrap());
    fs::write(dir.join("config/profiles/work.toml"), b"name = \"evil\"\n").unwrap();
    let error = verify_tree(&dir).unwrap_err();
    assert!(error.to_string().contains("content changed"), "{error}");
}

#[test]
fn documented_cp_restore_reproduces_original_state() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed(temp.path());
    let (dir, ..) = created(ensure("symbrain", &roots(temp.path()), &store).unwrap());
    // Simulate a damaging first Rust run: state replaced and partly deleted.
    fs::remove_dir_all(temp.path().join("config/symbrain")).unwrap();
    fs::write(
        temp.path().join("data/symbrain/memory/default.db"),
        b"corrupt",
    )
    .unwrap();
    assert!(verify_restore(&dir).is_err(), "damage must be detected");
    for (label, original) in [("config", "config/symbrain"), ("data", "data/symbrain")] {
        let original = temp.path().join(original);
        fs::create_dir_all(&original).unwrap();
        let status = Command::new("cp")
            .arg("-Rp")
            .arg(dir.join(label).join("."))
            .arg(&original)
            .status()
            .unwrap();
        assert!(status.success());
    }
    verify_restore(&dir).unwrap();
}

#[test]
fn unreadable_state_fails_closed_without_marker_or_partial_backup() {
    let temp = tempfile::tempdir().unwrap();
    let store = seed(temp.path());
    let secret = temp.path().join("config/symbrain/config.toml");
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&secret).is_ok() {
        // Running as root: permission bits do not deny reads; nothing to prove.
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
        return;
    }
    let result = ensure("symbrain", &roots(temp.path()), &store);
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(result.is_err());
    assert!(!store.join("symbrain.json").exists());
    let left: Vec<_> = fs::read_dir(&store)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, [".symbrain.lock"], "partial backup removed");
    let (dir, ..) = created(ensure_when_free(&roots(temp.path()), &store));
    verify_tree(&dir).unwrap();
}
