//! Prepared semantic regressions. Not executed at the source-only checkpoint.

use super::*;
use crate::Store;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn time(raw: &str) -> SyncTime {
    serde_json::from_str(&format!("\"{raw}\"")).expect("owned fixture time")
}
fn memory(id: &str, at: SyncTime) -> SyncMemory {
    SyncMemory {
        id: id.into(),
        content: "synthetic context".into(),
        scope: "global".into(),
        created_at: at,
        updated_at: at,
        tier: "long_term".into(),
        ..SyncMemory::default()
    }
}
fn options(store: &Store, pull: bool, push: bool) -> Options<'_> {
    Options {
        remote: "http://127.0.0.1:1/owned/",
        store: Some(store),
        pull,
        push,
        encrypted_relay: false,
        passphrase: "",
        allow_insecure_http: false,
        page_limit: 2,
        timeout: Duration::from_secs(60),
        quantize_binary: false,
    }
}

#[derive(Default)]
struct Transcript {
    pages: VecDeque<Result<Changes, SyncError>>,
    pulls: Vec<(SyncTime, String, i64)>,
    pushes: Vec<(Vec<String>, Vec<String>)>,
}
impl SyncTransport for Transcript {
    fn validate(&self, _remote: &str, _allow: bool) -> Result<(), SyncError> {
        Ok(())
    }
    fn changes(
        &mut self,
        _context: &RunContext,
        since: SyncTime,
        cursor: &str,
        limit: i64,
    ) -> Result<Changes, SyncError> {
        self.pulls.push((since, cursor.into(), limit));
        self.pages.pop_front().expect("scripted request")
    }
    fn apply(
        &mut self,
        _context: &RunContext,
        memories: &[SyncMemory],
        deleted: &[DeletedMemory],
    ) -> Result<ApplyResult, SyncError> {
        self.pushes.push((
            memories.iter().map(|row| row.id.clone()).collect(),
            deleted.iter().map(|row| row.id.clone()).collect(),
        ));
        Ok(ApplyResult {
            applied: i64::try_from(memories.len()).expect("bounded"),
            deleted: i64::try_from(deleted.len()).expect("bounded"),
            ..ApplyResult::default()
        })
    }
    fn relay_pull(
        &mut self,
        _context: &RunContext,
        _since: SyncTime,
        _limit: i64,
    ) -> Result<RelayChanges, SyncError> {
        panic!("no relay request scripted")
    }
    fn relay_push(
        &mut self,
        _context: &RunContext,
        _blobs: &[RelayBlob],
    ) -> Result<RelayPushResult, SyncError> {
        panic!("no relay request scripted")
    }
}

#[test]
fn failed_second_page_keeps_first_page_and_old_cursor() {
    let store = Store::open_in_memory().expect("owned store");
    let previous = time("2026-01-01T00:00:00Z");
    let later = time("2026-01-02T00:00:00Z");
    let opts = options(&store, true, false);
    store
        .set_sync_cursor(opts.remote, previous)
        .expect("cursor");
    let mut transport = Transcript {
        pages: VecDeque::from([
            Ok(Changes {
                memories: Some(vec![memory("first", later)]),
                next_cursor: "next&opaque".into(),
                server_time: later,
                ..Changes::default()
            }),
            Err(SyncError("owned page failure".into())),
        ]),
        ..Transcript::default()
    };
    let error = run(&opts, &mut transport, None, None).expect_err("second page fails");
    assert_eq!(
        error.to_string(),
        format!("pull from {}: owned page failure", opts.remote)
    );
    assert_eq!(store.sync_cursor(opts.remote).expect("cursor"), previous);
    assert_eq!(
        store
            .sync_memories_since(SyncTime::default(), "", 10)
            .expect("row")
            .len(),
        1
    );
    assert_eq!(
        transport.pulls[1],
        (SyncTime::default(), "next&opaque".into(), 2)
    );
    // A rerun sees the old cursor; equal-clock replays skip but still advance.
    let mut retry = Transcript {
        pages: VecDeque::from([Ok(Changes {
            memories: Some(vec![memory("first", later)]),
            server_time: later,
            ..Changes::default()
        })]),
        ..Transcript::default()
    };
    let result = run(&opts, &mut retry, None, None).expect("retry");
    assert_eq!(result.pulled_memories_applied, 0);
    assert_eq!(result.cursor, later);
    assert_eq!(retry.pulls[0].0, previous);
}

#[test]
fn equal_delete_wins_and_creation_identity_survives_newer_update() {
    let store = Store::open_in_memory().expect("owned store");
    let before = time("2026-01-01T00:00:00Z");
    let after = time("2026-01-02T00:00:00Z");
    let mut row = memory("owned", before);
    row.created_by = "original author".into();
    row.valid_from = Some(before);
    assert!(store.upsert_sync_memory(&row, false).expect("insert"));
    row.updated_at = after;
    row.created_by = "peer changed creation".into();
    row.valid_from = Some(after);
    assert!(store.upsert_sync_memory(&row, false).expect("update"));
    let stored = store
        .sync_memories_since(SyncTime::default(), "", 10)
        .expect("read");
    assert_eq!(stored[0].created_by, "original author");
    assert_eq!(stored[0].valid_from, Some(before));
    assert!(
        !store
            .apply_sync_delete(&DeletedMemory {
                id: row.id.clone(),
                deleted_at: before
            })
            .expect("old delete")
    );
    assert!(
        store
            .apply_sync_delete(&DeletedMemory {
                id: row.id,
                deleted_at: after
            })
            .expect("equal delete")
    );
    assert!(
        store
            .sync_memories_since(SyncTime::default(), "", 10)
            .expect("read")
            .is_empty()
    );
}

#[test]
fn hidden_list_rows_are_synced_except_literal_sync_exclude() {
    let store = Store::open_in_memory().expect("owned store");
    let at = time("2026-01-01T00:00:00Z");
    for (id, excluded) in [("staged", false), ("excluded", true)] {
        let mut row = memory(id, at);
        row.review_status = "staged".into();
        row.retired_at = Some(at);
        row.consolidation_status = "archived".into();
        if excluded {
            row.metadata = Some([("sync_exclude".into(), "true".into())].into());
        }
        store.upsert_sync_memory(&row, false).expect("insert");
    }
    let mut transport = Transcript::default();
    let result = run(&options(&store, false, true), &mut transport, None, None).expect("push");
    assert_eq!(result.pushed_memories, 1);
    assert_eq!(transport.pushes, vec![(vec!["staged".into()], vec![])]);
}

#[test]
fn empty_push_can_reset_cursor_even_with_expired_caller_context() {
    let store = Store::open_in_memory().expect("owned store");
    let opts = options(&store, false, true);
    store
        .set_sync_cursor(opts.remote, time("2026-01-01T00:00:00Z"))
        .expect("cursor");
    let expired = RunContext::with_deadline(Instant::now() - Duration::from_secs(1));
    let mut transport = Transcript::default();
    let result = run(&opts, &mut transport, None, Some(&expired)).expect("no request needed");
    assert_eq!(result.cursor, SyncTime::default());
    assert!(transport.pushes.is_empty());
}

#[test]
fn apply_aliases_do_not_override_bad_or_null_canonical_counters() {
    let result: ApplyResult = serde_json::from_str(
        r#"{"skippedInvalidScope":17,"skipped_invalid_scope":null,"skipped_invalid_id":"bad","skippedInvalidID":12,"applied":-2}"#)
        .expect("Go-style ignored integer decoding errors");
    assert_eq!(result.skipped_invalid_scope, 0);
    assert_eq!(result.skipped_invalid_id, 0);
    assert_eq!(result.applied, -2);
}

#[test]
fn wire_clocks_trim_nanos_and_plain_since_drops_them() {
    let at = time("2026-01-01T12:00:00.123400+02:00");
    assert_eq!(at.wire(), "2026-01-01T12:00:00.1234+02:00");
    assert_eq!(at.plain_since(), "2026-01-01T10:00:00Z");
    assert_eq!(at.relay_since(), "2026-01-01T10:00:00.1234Z");
    assert_eq!(SyncTime::parse_sqlite(&at.sqlite()), Some(at));
}
