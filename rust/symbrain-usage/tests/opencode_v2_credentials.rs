//! Synthetic native DB/WAL discovery through the public provider constructor, in bounded fresh processes.
#![cfg(unix)]
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use symbrain_usage::{ProviderSelection, selected_providers};

fn inventory(root: &Path) -> BTreeMap<String, (u32, u64, i64, i64, Vec<u8>)> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let m = fs::symlink_metadata(&path).unwrap();
        if m.is_dir() {
            result.extend(inventory(&path));
            continue;
        }
        let content = if m.is_file() {
            Sha256::digest(fs::read(&path).unwrap()).to_vec()
        } else if m.file_type().is_symlink() {
            fs::read_link(&path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            assert!(m.file_type().is_fifo());
            Vec::new()
        };
        result.insert(
            path.to_str().unwrap().into(),
            (m.mode(), m.len(), m.mtime(), m.mtime_nsec(), content),
        );
    }
    result
}

fn child() {
    let selection = ProviderSelection::parse("opencode").unwrap();
    assert!(!symbrain_usage::needs_go_fallback_for_selection(&selection));
    let providers = selected_providers(&selection);
    let outcome = std::env::var("OPENCODE_V2_OUTCOME").unwrap();
    if outcome == "none" || outcome == "error" {
        assert!(!providers[0].configured);
        assert_eq!(
            providers[0]
                .auth_status
                .detail
                .contains("credential resolution failed"),
            outcome == "error"
        );
        return;
    }
    assert!(providers[0].configured);
    let transport = std::sync::Arc::new(symbrain_usage::FixtureTransport::new(
        [(
            "opencode".into(),
            symbrain_usage::Response {
                status: 200,
                headers: BTreeMap::new(),
                body: br#"{"usage":{"weekly":{"percent":0}}}"#.to_vec(),
            },
        )]
        .into(),
    ));
    let report = symbrain_usage::Service::with_transport(providers, transport.clone()).report();
    assert!(report.providers[0].snapshot.is_some());
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let expected = if outcome == "current" {
        "Bearer synthetic-current"
    } else {
        "Bearer synthetic-legacy"
    };
    assert_eq!(requests[0].headers.get("Authorization").unwrap(), expected);
}

#[test]
fn opencode_v2_fresh_processes() {
    if std::env::var_os("OPENCODE_V2_OUTCOME").is_some() {
        child();
        return;
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/opencode-v2-synthetic.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 45);
    let mut cases = cases.clone();
    cases.extend(wal_reuse_cases());
    for case in &cases {
        let id = case["id"].as_str().unwrap();
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir(&data).unwrap();
        let path = data.join("opencode.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(fixture["schema"].as_str().unwrap())
            .unwrap();
        if case["imported"].as_bool().unwrap_or(true) {
            db.execute_batch(
                "INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1);",
            )
            .unwrap();
        }
        let wal = case["wal"].as_bool().unwrap_or(false);
        if wal {
            db.execute_batch("INSERT INTO credential VALUES('baseline','opencode-go','{\"key\":\"synthetic-stale\"}',1,0); PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        }
        if let Some(sql) = case["sql"].as_str() {
            db.execute_batch(sql).unwrap();
        } else if wal {
            db.execute_batch("DELETE FROM credential;").unwrap();
        }
        if case["mutation"]
            .as_str()
            .is_some_and(|m| m.starts_with("wal-reuse-"))
        {
            restart_wal(&db, &path, case["mutation"].as_str().unwrap());
        }
        let db = if wal {
            Some(db)
        } else {
            db.close().unwrap();
            None
        };
        fs::write(
            data.join("auth.json"),
            br#"{"opencode-go":{"key":"synthetic-legacy"}}"#,
        )
        .unwrap();
        mutate(case, root.path(), &data, &path);
        let snapshots = root.path().join("private-snapshots");
        fs::create_dir(&snapshots).unwrap();
        let before = inventory(root.path());
        let mut process = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "opencode_v2_fresh_processes", "--nocapture"])
            .env_clear()
            .env("HOME", root.path())
            .env("PATH", "")
            .env("TMPDIR", &snapshots)
            .env("OPENCODE_DATA_DIR", &data)
            .env("OPENCODE_V2_OUTCOME", case["outcome"].as_str().unwrap())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = process.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(10) {
                process.kill().unwrap();
                process.wait().unwrap();
                panic!("bounded discovery child timed out: {id}");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "discovery child failed: {id}");
        assert_eq!(
            fs::read_dir(&snapshots).unwrap().count(),
            0,
            "private snapshot leaked: {id}"
        );
        assert_eq!(before, inventory(root.path()), "source changed: {id}");
        if let Some(db) = db {
            db.close().unwrap();
        }
    }
}

fn mutate(case: &serde_json::Value, root: &Path, data: &Path, path: &Path) {
    let wal_path = data.join("opencode.db-wal");
    match case["mutation"].as_str().unwrap_or("") {
        "orphan-wal" => {
            fs::remove_file(path).unwrap();
            fs::write(&wal_path, []).unwrap();
        }
        "dir-symlink" => {
            let real = root.join("real");
            fs::rename(data, &real).unwrap();
            std::os::unix::fs::symlink(real, data).unwrap();
        }
        "many-entries" => {
            for index in 0..256 {
                fs::write(data.join(format!("extra-{index}")), []).unwrap();
            }
        }

        "remove-shm" => fs::remove_file(data.join("opencode.db-shm")).unwrap(),
        "corrupt-db" => fs::write(path, b"not sqlite").unwrap(),
        mutation @ ("wal-header" | "wal-frame" | "wal-truncate") => {
            let mut bytes = fs::read(&wal_path).unwrap();
            assert!(bytes.len() > 64);
            if mutation == "wal-truncate" {
                bytes.pop();
            } else if mutation == "wal-header" {
                bytes[0] ^= 1;
            } else {
                let n = bytes.len();
                bytes[n - 1] ^= 1;
            }
            fs::write(&wal_path, bytes).unwrap();
        }
        "second-channel" => fs::write(data.join("opencode-next.db"), []).unwrap(),
        "preview" => fs::rename(path, data.join("opencode-next.db")).unwrap(),
        mutation @ ("db-symlink" | "db-fifo") => {
            let outside = root.join("outside");
            fs::rename(path, &outside).unwrap();
            if mutation == "db-symlink" {
                std::os::unix::fs::symlink(outside, path).unwrap();
            } else {
                assert!(
                    Command::new("/usr/bin/mkfifo")
                        .arg(path)
                        .status()
                        .unwrap()
                        .success()
                );
            }
        }
        "wal-symlink" => std::os::unix::fs::symlink(path, &wal_path).unwrap(),
        "wal-fifo" => assert!(
            Command::new("/usr/bin/mkfifo")
                .arg(&wal_path)
                .status()
                .unwrap()
                .success()
        ),
        "journal" => fs::write(data.join("opencode.db-journal"), []).unwrap(),
        "oversized-db" => fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_len((64 << 20) + 1)
            .unwrap(),
        "" => (),
        m if m.starts_with("wal-reuse-") => (),
        _ => panic!("unknown mutation"),
    }
}

fn restart_wal(db: &rusqlite::Connection, path: &Path, mode: &str) {
    db.execute_batch("INSERT INTO credential VALUES('reuse','opencode-go','{\"key\":\"synthetic-stale\"}',1,1); CREATE TABLE padding(value BLOB); INSERT INTO padding VALUES(zeroblob(200000));").unwrap();
    let wal = path.with_file_name("opencode.db-wal");
    let old = fs::read(&wal).unwrap();
    let checkpoint: (i64, i64, i64) = db
        .query_row("PRAGMA wal_checkpoint(RESTART)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(checkpoint.0, 0);
    assert_eq!(checkpoint.1, checkpoint.2);
    db.execute_batch(if mode == "wal-reuse-none" {
        "DELETE FROM credential"
    } else {
        "UPDATE credential SET value='{\"key\":\"synthetic-current\"}',time_updated=2"
    })
    .unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM credential WHERE value='{\"key\":\"synthetic-current\"}'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, i64::from(mode != "wal-reuse-none"));
    let mut data = fs::read(&wal).unwrap();
    let size = usize::try_from(u32::from_be_bytes(data[8..12].try_into().unwrap())).unwrap() + 24;
    let mut end = 32;
    while end + size <= data.len() && data[end + 8..end + 16] == data[16..24] {
        end += size;
    }
    assert_eq!(data.len(), old.len());
    assert!(end > 32 && end < data.len());
    assert_eq!(data[end..], old[end..]);
    eprintln!(
        "normal WAL reuse: physical={} current_frames={} obsolete_frames={} current_salts={:x?} old_salts={:x?}",
        data.len(),
        (end - 32) / size,
        (data.len() - end) / size,
        &data[16..24],
        &old[16..24]
    );
    match mode {
        "wal-reuse-corrupt-current" => data[end - 1] ^= 1,
        "wal-reuse-wrong-tail-salt" => data[end + 8] ^= 0x40,
        "wal-reuse-current-after-tail" => {
            let salts: [u8; 8] = data[16..24].try_into().unwrap();
            let start = data.len() - size + 8;
            data[start..start + 8].copy_from_slice(&salts);
        }
        _ => return,
    }
    fs::write(wal, data).unwrap();
}

fn wal_reuse_cases() -> impl Iterator<Item = serde_json::Value> {
    ["current", "none", "corrupt-current", "wrong-tail-salt", "current-after-tail"].into_iter().map(|outcome| {
        let expected = if outcome == "current" || outcome == "none" { outcome } else { "error" };
        serde_json::json!({"id": format!("wal-reuse-{outcome}"), "wal": true, "outcome": expected, "mutation": format!("wal-reuse-{outcome}")})
    })
}
