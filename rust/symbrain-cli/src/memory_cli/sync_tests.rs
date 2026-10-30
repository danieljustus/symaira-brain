//! Native `memory sync` against a loopback relay served from a test thread.
//!
//! Shapes and wordings are pinned against the shipped client by the
//! `sync_relay` process family in `scripts/memory-cli-oracle`; these tests
//! keep the Rust units honest without a Go binary.

use super::*;
use rusqlite::Connection;
use std::io::Read;
use std::net::TcpListener;
use std::time::Duration;
use symbrain_memory::StoredCursor;
use tempfile::tempdir;

/// Serves `responses` in order, one per connection, and returns the requests.
fn relay(responses: Vec<Vec<u8>>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("address").port();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for response in responses {
            let (mut connection, _) = listener.accept().expect("accept");
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("timeout");
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = connection.read(&mut chunk).expect("read request");
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
            }
            seen.push(String::from_utf8_lossy(&request).into_owned());
            connection.write_all(&response).expect("respond");
        }
        seen
    });
    (port, handle)
}

fn reply(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn sync(args: &[String], format: OutputFormat) -> (Option<u8>, String, String) {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run_sync(&args, &mut stdout, &mut stderr, format);
    (
        code,
        String::from_utf8(stdout).expect("utf-8"),
        String::from_utf8(stderr).expect("utf-8"),
    )
}

fn invocation(port: u16, database: &std::path::Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "sync".to_owned(),
        format!("--remote=http://127.0.0.1:{port}"),
        "--db".to_owned(),
        database.display().to_string(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    args
}

#[test]
fn empty_exchange_sends_the_shipped_request_and_stores_the_cursor() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let body = r#"{"memories":[],"deleted":[],"server_time":"2026-10-26T01:02:03.456Z"}"#;
    let (port, requests) = relay(vec![reply("200 OK", body), reply("200 OK", body)]);

    let (code, stdout, stderr) = sync(
        &invocation(port, &database, &["--token", "tok"]),
        OutputFormat::Table,
    );
    assert_eq!((code, stderr.as_str()), (Some(exit::OK), ""), "{stdout}");
    assert!(stdout.contains("Mode:              both\n"), "{stdout}");
    assert!(
        stdout.contains("Cursor:            2026-10-26T01:02:03Z\n"),
        "{stdout}"
    );

    let (code, stdout, _) = sync(&invocation(port, &database, &[]), OutputFormat::Json);
    assert_eq!(code, Some(exit::OK));
    assert!(
        stdout.contains(r#""cursor":"2026-10-26T01:02:03.456Z""#),
        "{stdout}"
    );

    let requests = requests.join().expect("relay");
    let first = requests[0].to_ascii_lowercase();
    assert!(
        requests[0].starts_with("GET /api/sync/changes?limit=500 HTTP/1.1\r\n"),
        "{}",
        requests[0]
    );
    assert!(first.contains("authorization: bearer tok\r\n"), "{first}");
    assert!(
        first.contains("user-agent: go-http-client/1.1\r\n"),
        "{first}"
    );
    assert!(first.contains("accept-encoding: gzip\r\n"), "{first}");
    assert!(!first.contains("\r\naccept:"), "{first}");
    assert!(
        requests[1].starts_with(
            "GET /api/sync/changes?limit=500&since=2026-10-26T01%3A02%3A03Z HTTP/1.1\r\n"
        ),
        "{}",
        requests[1]
    );

    let store = Store::open(&database).expect("store");
    let stored = store
        .sync_cursor(&format!("http://127.0.0.1:{port}"))
        .expect("cursor");
    assert!(matches!(stored, StoredCursor::At(_)), "{stored:?}");
}

#[test]
fn error_statuses_use_the_shipped_wording_and_keep_the_cursor_unwritten() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let (port, requests) = relay(vec![
        reply(
            "503 Service Unavailable",
            r#"{"error":"busy","code":"E_BUSY"}"#,
        ),
        reply("500 Oops", "<html>"),
        reply("200 OK", ""),
    ]);
    let args = invocation(port, &database, &["--pull"]);
    let remote = format!("http://127.0.0.1:{port}");
    for expected in [
        format!("symbrain memory sync: pull from {remote}: remote returned E_BUSY (503): busy\n"),
        format!("symbrain memory sync: pull from {remote}: remote returned status 500: \n"),
        format!("symbrain memory sync: pull from {remote}: decode remote response: EOF\n"),
    ] {
        let (code, stdout, stderr) = sync(&args, OutputFormat::Table);
        assert_eq!((code, stdout.as_str()), (Some(exit::GENERIC), ""));
        assert_eq!(stderr, expected);
    }
    requests.join().expect("relay");
    let store = Store::open(&database).expect("store");
    assert_eq!(
        store.sync_cursor(&remote).expect("cursor"),
        StoredCursor::Absent
    );
}

#[test]
fn replies_that_carry_data_are_handed_off_without_output() {
    for body in [
        r#"{"memories":[{"id":5}]}"#,
        r#"{"memories":[{"id":"a","embedding":[0.5]}]}"#,
        r#"{"server_time":"2026-10-26T03:02:03+02:00"}"#,
    ] {
        let directory = tempdir().expect("directory");
        let database = directory.path().join("memory.db");
        let (port, requests) = relay(vec![reply("200 OK", body)]);
        let (code, stdout, stderr) = sync(
            &invocation(port, &database, &["--pull"]),
            OutputFormat::Table,
        );
        assert_eq!(
            (code, stdout.as_str(), stderr.as_str()),
            (None, "", ""),
            "{body}"
        );
        assert_eq!(requests.join().expect("relay").len(), 1);
    }
}

#[test]
fn push_only_without_local_changes_is_offline_and_resets_the_cursor() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let (code, stdout, _) = sync(&invocation(9, &database, &["--push"]), OutputFormat::Json);
    assert_eq!(code, Some(exit::OK));
    assert!(
        stdout.contains(r#""mode":"push","encrypted_relay":false,"cursor":"0001-01-01T00:00:00Z""#),
        "{stdout}"
    );
}

#[test]
fn remotes_outside_the_proven_subset_hand_off_before_any_work() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("never.db");
    for remote in [
        "https://127.0.0.1:9",
        "http://localhost:9",
        "http://127.0.0.2:9",
        "http://127.0.0.1",
        "http://127.0.0.1:80",
        "http://127.0.0.1:09",
        "HTTP://127.0.0.1:9",
        "http://127.0.0.1:9?x=1",
        "http://127.0.0.1:9/a/../b",
        "http://127.0.0.1:9/a//b",
        "http://u@127.0.0.1:9",
    ] {
        let args = [
            "sync".to_owned(),
            format!("--remote={remote}"),
            "--db".to_owned(),
            database.display().to_string(),
        ];
        let (code, stdout, stderr) = sync(&args, OutputFormat::Table);
        assert_eq!(
            (code, stdout.as_str(), stderr.as_str()),
            (None, "", ""),
            "{remote}"
        );
    }
    let relay_args = invocation(9, &database, &["--encrypted-relay", "--relay-passphrase=p"]);
    assert_eq!(sync(&relay_args, OutputFormat::Table).0, None);
    for odd_token in ["a b", " a", "a\u{e4}"] {
        let args = invocation(9, &database, &["--token", odd_token]);
        assert_eq!(sync(&args, OutputFormat::Table).0, None, "{odd_token:?}");
    }
    for timeout in ["500ms", "61s", "2m", "1h", "0", "1.5s", "01s", "1ns"] {
        let args = invocation(9, &database, &["--timeout", timeout]);
        assert_eq!(sync(&args, OutputFormat::Table).0, None, "{timeout}");
    }
    assert!(
        !database.exists(),
        "a hand-off before the database is opened must not create it"
    );
}

#[test]
fn unreadable_stored_cursors_and_pending_changes_are_handed_off() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let store = Store::open(&database).expect("store");
    drop(store);
    let remote = "http://127.0.0.1:9";
    let connection = Connection::open(&database).expect("connection");
    connection
        .execute(
            "INSERT INTO sync_state(remote,last_sync) VALUES(?,'2026-10-26 03:02:03 +0200 +0200')",
            [remote],
        )
        .expect("seed");
    let args = invocation(9, &database, &["--pull"]);
    assert_eq!(sync(&args, OutputFormat::Table).0, None);
    connection
        .execute(
            "UPDATE sync_state SET last_sync='2000-01-01 00:00:00 +0000 UTC'",
            [],
        )
        .expect("reset");
    connection
        .execute(
            "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) VALUES('m','c','global','reference','2000-01-01 00:00:00 +0000 UTC','2026-10-26 00:00:00 +0000 UTC','{}',X'')",
            [],
        )
        .expect("memory");
    // Both directions: the push half finds the newer local memory.
    let both = invocation(9, &database, &[]);
    assert_eq!(
        sync(&both, OutputFormat::Table).0,
        None,
        "pending local memory"
    );
}

#[test]
fn flag_timeouts_follow_the_claimed_spellings() {
    use super::sync_http::timeout_from_flag;
    assert_eq!(timeout_from_flag(b""), Some(Duration::from_secs(60)));
    assert_eq!(timeout_from_flag(b"1s"), Some(Duration::from_secs(1)));
    assert_eq!(timeout_from_flag(b"60s"), Some(Duration::from_secs(60)));
    assert_eq!(timeout_from_flag(b"1m"), Some(Duration::from_secs(60)));
    for unclaimed in [
        "0s", "61s", "2m", "1h", "1.5s", "500ms", "-1s", "+1s", "s", "05s", "1000s",
    ] {
        assert_eq!(timeout_from_flag(unclaimed.as_bytes()), None, "{unclaimed}");
    }
}
