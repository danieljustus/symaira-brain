//! #971: push pagination over `sync_oplog` tombstones (and live rows) written
//! by the schema triggers — never pinned timestamps — terminates and sends
//! every id exactly once, for the plain and the encrypted-relay push.
//!
//! The triggers store millisecond `T…Z` text while the cursor is bound in the
//! Go layout, so hundreds of rows share one timestamp and the (ts, id) cursor
//! has to compare chronologically and break ties by id.
#![allow(clippy::pedantic)]

use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DELETED: usize = 450;
const LIVE: usize = 250;
/// A terminating push needs ceil(450 / 200) = 3 batches; anything near this
/// bound means the cursor is not advancing.
const MAX_REQUESTS: usize = 12;

type Requests = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

struct Remote {
    port: u16,
    bodies: Requests,
    stop: Arc<AtomicBool>,
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut chunk = [0_u8; 65536];
    let end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..count]);
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut body = data[end + 4..].to_vec();
    let target = head.lines().next()?.split(' ').nth(1)?.to_owned();
    let lower = head.to_ascii_lowercase();
    if let Some(length) = lower
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        while body.len() < length {
            let count = stream.read(&mut chunk).ok()?;
            if count == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..count]);
        }
    } else if lower.contains("transfer-encoding: chunked") {
        while !body.ends_with(b"0\r\n\r\n") {
            let count = stream.read(&mut chunk).ok()?;
            if count == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..count]);
        }
        body = dechunk(&body);
    }
    Some((target, body))
}

fn dechunk(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.windows(2).position(|w| w == b"\r\n") {
        let size =
            usize::from_str_radix(String::from_utf8_lossy(&rest[..at]).trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.extend_from_slice(&rest[at + 2..at + 2 + size]);
        rest = &rest[at + 2 + size + 2..];
    }
    out
}

impl Remote {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, halt) = (bodies.clone(), stop.clone());
        std::thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                let Some((target, body)) = read_request(&mut stream) else {
                    continue;
                };
                let count = {
                    let mut seen = seen.lock().unwrap();
                    seen.push((target.clone(), body.clone()));
                    seen.len()
                };
                let (status, reply) = if count > MAX_REQUESTS {
                    (500, r#"{"error":"push did not terminate"}"#.to_owned())
                } else if target.starts_with("/api/sync/apply") {
                    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let len = |key: &str| parsed[key].as_array().map_or(0, Vec::len);
                    (
                        200,
                        format!(
                            r#"{{"applied":{},"deleted":{}}}"#,
                            len("memories"),
                            len("deleted")
                        ),
                    )
                } else {
                    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let stored = parsed["blobs"].as_array().map_or(0, Vec::len);
                    (200, format!(r#"{{"stored":{stored}}}"#))
                };
                let text = format!(
                    "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = stream.write_all(text.as_bytes());
            }
        });
        Self { port, bodies, stop }
    }
}

fn seed(db: &Path) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    drop(symbrain_memory::Store::open(db).expect("create store"));
    let mut conn = rusqlite::Connection::open(db).unwrap();
    let tx = conn.transaction().unwrap();
    for index in 0..(DELETED + LIVE) {
        tx.execute(
            "INSERT INTO memories (id, content, scope, metadata, embedding, created_at, updated_at) VALUES (?, ?, 'global', '{}', '[]', '2026-10-10T08:00:00Z', ?)",
            rusqlite::params![format!("m-{index:04}"), format!("content {index}"), "2026-10-10T08:00:00Z"],
        )
        .unwrap();
    }
    tx.execute("DELETE FROM memories WHERE id < 'm-0450'", [])
        .unwrap();
    tx.commit().unwrap();
    let deletes: i64 = conn
        .query_row(
            "SELECT count(*) FROM sync_oplog WHERE op = 'delete'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deletes as usize, DELETED, "triggers wrote the tombstones");
}

fn run_push(extra: &[&str]) -> (Remote, std::process::Output) {
    let home = tempfile::tempdir().unwrap();
    let db = home.path().join("data/m.db");
    seed(&db);
    let remote = Remote::start();
    let url = format!("http://127.0.0.1:{}", remote.port);
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("cfg"))
        .env("XDG_DATA_HOME", home.path().join("xdg"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .args([
            "memory",
            "sync",
            "--push",
            "--allow-insecure-http",
            "--remote",
            &url,
            "--db",
        ])
        .arg(&db)
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "push did not terminate"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    (remote, output)
}

fn ids_sent(remote: &Remote, key: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for (target, body) in remote.bodies.lock().unwrap().iter() {
        if body.is_empty() {
            // The relay mode lists the remote before pushing.
            assert!(target.starts_with("/api/sync/relay?"), "{target}");
            continue;
        }
        let parsed: Value = serde_json::from_slice(body).unwrap();
        for item in parsed[key].as_array().into_iter().flatten() {
            ids.push(item["id"].as_str().unwrap().to_owned());
        }
    }
    ids
}

fn assert_each_once(ids: &[String], expected: usize) {
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "an id was sent twice");
    assert_eq!(ids.len(), expected);
}

#[test]
fn plain_push_sends_trigger_written_tombstones_once_and_terminates() {
    let (remote, output) = run_push(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_each_once(&ids_sent(&remote, "deleted"), DELETED);
    assert_each_once(&ids_sent(&remote, "memories"), LIVE);
    assert!(remote.bodies.lock().unwrap().len() <= 5);
}

#[test]
fn relay_push_sends_trigger_written_tombstones_once_and_terminates() {
    let (remote, output) = run_push(&["--encrypted-relay", "--relay-passphrase", "test-pass"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ids = ids_sent(&remote, "blobs");
    let (tombstones, memories): (Vec<_>, Vec<_>) = ids
        .iter()
        .cloned()
        .partition(|id| id.starts_with("tombstone:"));
    assert_each_once(&tombstones, DELETED);
    assert_each_once(&memories, LIVE);
    assert!(remote.bodies.lock().unwrap().len() <= 5);
}
