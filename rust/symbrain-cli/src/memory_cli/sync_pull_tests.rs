//! Native `memory sync` with replies that carry data, against a loopback relay
//! served from a test thread. The process-level proof against the shipped
//! client is the `sync_relay` family of `scripts/memory-cli-oracle`.

use super::*;
use rusqlite::Connection;
use std::io::Read;
use std::net::TcpListener;
use std::time::Duration;
use tempfile::tempdir;

/// Serves `responses` in order, one per connection, and returns the targets.
fn relay(responses: Vec<String>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("address").port();
    let handle = std::thread::spawn(move || {
        let mut targets = Vec::new();
        for body in responses {
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
            let text = String::from_utf8_lossy(&request).into_owned();
            targets.push(text.lines().next().unwrap_or_default().to_owned());
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            connection.write_all(reply.as_bytes()).expect("respond");
        }
        targets
    });
    (port, handle)
}

fn run(port: u16, database: &std::path::Path, extra: &[&str]) -> (Option<u8>, String, String) {
    let mut args = vec![
        "sync".to_owned(),
        format!("--remote=http://127.0.0.1:{port}"),
        "--db".to_owned(),
        database.display().to_string(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run_sync(&args, &mut stdout, &mut stderr, OutputFormat::Json);
    (
        code,
        String::from_utf8(stdout).expect("utf-8"),
        String::from_utf8(stderr).expect("utf-8"),
    )
}

fn page(memories: &str, deleted: &str, time: &str, next: &str) -> String {
    format!(
        r#"{{"memories":[{memories}],"deleted":[{deleted}],"server_time":"{time}","next_cursor":"{next}"}}"#
    )
}

fn scalar(database: &std::path::Path, sql: &str) -> String {
    Connection::open(database)
        .expect("connection")
        .query_row(sql, [], |row| row.get::<_, String>(0))
        .expect("scalar")
}

#[test]
fn pages_are_fetched_by_cursor_applied_and_counted() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let first = page(
        r#"{"id":"a","content":"one","scope":"global","created_at":"2026-10-26T01:00:00Z","updated_at":"2026-10-26T01:00:00Z"}"#,
        "",
        "2026-10-26T01:10:00Z",
        "p 2",
    );
    let second = page(
        r#"{"id":"b","content":"two","scope":"project","updated_at":"2026-10-26T01:20:00.5Z"}"#,
        r#"{"id":"a","deleted_at":"2026-10-26T01:30:00Z"}"#,
        "2026-10-26T01:05:00Z",
        "",
    );
    let (port, targets) = relay(vec![first, second]);
    let (code, stdout, stderr) = run(port, &database, &["--pull"]);
    assert_eq!((code, stderr.as_str()), (Some(exit::OK), ""), "{stdout}");
    assert!(
        stdout.contains(r#""cursor":"2026-10-26T01:30:00Z""#)
            && stdout.contains(r#""pulled_memories_applied":2,"pulled_deletes_applied":1"#),
        "{stdout}"
    );
    assert_eq!(
        targets.join().expect("relay"),
        [
            "GET /api/sync/changes?limit=500 HTTP/1.1",
            "GET /api/sync/changes?cursor=p+2&limit=500 HTTP/1.1"
        ]
    );
    assert_eq!(
        scalar(&database, "SELECT group_concat(id) FROM memories"),
        "b"
    );
    assert_eq!(
        scalar(&database, "SELECT updated_at FROM memories WHERE id='b'"),
        "2026-10-26 01:20:00.5 +0000 UTC"
    );
    assert_eq!(
        scalar(&database, "SELECT last_sync FROM sync_state"),
        "2026-10-26 01:30:00 +0000 UTC"
    );
}

#[test]
fn a_failing_second_page_keeps_the_first_page_and_the_old_cursor() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let first = page(
        r#"{"id":"a","content":"one","scope":"global","updated_at":"2026-10-26T01:00:00Z"}"#,
        "",
        "2026-10-26T01:10:00Z",
        "p2",
    );
    let (port, targets) = relay(vec![first, "<html>".to_owned()]);
    let (code, stdout, stderr) = run(port, &database, &["--pull"]);
    assert_eq!((code, stdout.as_str()), (Some(exit::GENERIC), ""));
    assert!(
        stderr.contains(
            "decode remote response: invalid character '<' looking for beginning of value"
        ),
        "{stderr}"
    );
    assert_eq!(targets.join().expect("relay").len(), 2);
    assert_eq!(scalar(&database, "SELECT id FROM memories"), "a");
    let rows = Connection::open(&database)
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM sync_state", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("count");
    assert_eq!(rows, 0);
}

#[test]
fn absent_row_delete_blocks_resurrection_in_later_pages_and_pulls() {
    for later_pull in [false, true] {
        let directory = tempdir().expect("directory");
        let database = directory.path().join("memory.db");
        let delete = page(
            "",
            r#"{"id":"gone","deleted_at":"2026-10-26T02:00:00Z"}"#,
            "2026-10-26T03:00:00Z",
            if later_pull { "" } else { "p2" },
        );
        let stale = page(
            r#"{"id":"gone","content":"stale","updated_at":"2026-10-26T01:00:00Z"}"#,
            "",
            "2026-10-26T03:00:00Z",
            "",
        );
        let fresh = page(
            r#"{"id":"gone","content":"fresh","updated_at":"2026-10-26T02:00:01Z"}"#,
            "",
            "2026-10-26T03:00:00Z",
            "",
        );
        let (port, targets) = relay(vec![delete, stale, fresh]);
        for _ in 0..if later_pull { 2 } else { 1 } {
            let (code, stdout, stderr) = run(port, &database, &["--pull"]);
            assert_eq!((code, stderr.as_str()), (Some(exit::OK), ""), "{stdout}");
            assert!(
                stdout.contains(r#""pulled_memories_applied":0,"pulled_deletes_applied":0"#),
                "{stdout}"
            );
        }
        assert_eq!(
            scalar(&database, "SELECT CAST(COUNT(*) AS TEXT) FROM memories"),
            "0"
        );
        assert_eq!(
            scalar(
                &database,
                "SELECT ts FROM sync_oplog WHERE memory_id='gone' ORDER BY event_id DESC LIMIT 1"
            ),
            "2026-10-26 02:00:00 +0000 UTC"
        );
        let (code, stdout, stderr) = run(port, &database, &["--pull"]);
        assert_eq!((code, stderr.as_str()), (Some(exit::OK), ""), "{stdout}");
        assert!(
            stdout.contains(r#""pulled_memories_applied":1,"pulled_deletes_applied":0"#),
            "{stdout}"
        );
        assert_eq!(
            scalar(&database, "SELECT content FROM memories WHERE id='gone'"),
            "fresh"
        );
        assert_eq!(targets.join().expect("relay").len(), 3);
    }
}

#[test]
fn a_pull_that_provokes_a_push_is_rolled_back_and_handed_off() {
    let directory = tempdir().expect("directory");
    let database = directory.path().join("memory.db");
    let body = page(
        r#"{"id":"a","content":"one","scope":"global","updated_at":"2026-10-26T01:00:00Z"}"#,
        "",
        "2026-10-26T01:10:00Z",
        "",
    );
    let (port, targets) = relay(vec![body]);
    let (code, stdout, stderr) = run(port, &database, &[]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (None, "", ""));
    assert_eq!(targets.join().expect("relay").len(), 1);
    assert_eq!(
        scalar(&database, "SELECT CAST(COUNT(*) AS TEXT) FROM memories"),
        "0"
    );
    assert_eq!(
        scalar(&database, "SELECT CAST(COUNT(*) AS TEXT) FROM sync_state"),
        "0"
    );
}
