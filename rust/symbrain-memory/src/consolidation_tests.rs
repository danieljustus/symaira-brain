//! Behavior tests for the consolidation engine, journal and undo with a fake
//! LLM, injected clock/ids and in-memory stores (#760). No network.

use crate::Store;
use crate::consolidation::Engine;
use crate::consolidation_apply::hash_embed;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::params;
use std::cell::{Cell, RefCell};

fn clock() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()
}

fn insert(store: &Store, id: &str, scope: &str, content: &str, day: u32, metadata: &str) {
    let created = crate::gotime::format(Utc.with_ymd_and_hms(2026, 5, day, 8, 0, 0).unwrap());
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at,consolidation_status,review_status) VALUES(?,?,?,?,'[]',?,?, 'raw','approved')",
            params![id, content, scope, metadata, created, created],
        )
        .unwrap();
}

fn cell(store: &Store, sql: &str, id: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    store
        .lock()
        .unwrap()
        .query_row(sql, [id], |r| r.get::<_, Option<String>>(0))
        .optional()
        .unwrap()
        .flatten()
}

/// Runs `body` with an engine whose LLM replies come from `replies`
/// (popped front to back) and which records every prompt it received.
fn with_engine<T>(
    replies: Vec<Result<String, String>>,
    pii: bool,
    body: impl FnOnce(&Engine<'_>, &RefCell<Vec<(String, String)>>) -> T,
) -> T {
    let queue = RefCell::new(
        replies
            .into_iter()
            .collect::<std::collections::VecDeque<_>>(),
    );
    let seen = RefCell::new(Vec::new());
    let counter = Cell::new(0_u32);
    let llm = |system: &str, user: &str| {
        seen.borrow_mut().push((system.to_owned(), user.to_owned()));
        queue
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err("no reply".into()))
    };
    let new_id = || {
        counter.set(counter.get() + 1);
        format!("new-{}", counter.get())
    };
    let engine = Engine {
        llm: &llm,
        embed: &hash_embed,
        pii_enabled: pii,
        prompt_mode: "chat".into(),
        now: &clock,
        new_id: &new_id,
    };
    body(&engine, &seen)
}

const MERGE_1_2: &str = r#"{"consolidated":[{"content":"Daniel prefers dark mode.","replaces_ids":["1","2","9"],"metadata":{"topic":"prefs"}}],"discarded_ids":["3","7"]}"#;

#[test]
fn lone_memory_is_marked_without_the_llm() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "a", "global", "only one", 1, "{}");
    let out = with_engine(vec![], false, |e, seen| {
        let out = store.run_consolidation(e, "", false).unwrap();
        assert!(seen.borrow().is_empty());
        out
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].replaced_id_to_new_id["a"], "a");
    let status = cell(
        &store,
        "SELECT consolidation_status FROM memories WHERE id=?",
        "a",
    );
    assert_eq!(status.as_deref(), Some("consolidated"));
    // The lone-memory path is journaled like any other run.
    assert!(store.last_completed_consolidation_run().unwrap().is_some());
}

#[test]
fn merge_discard_and_unmentioned_memories() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "m1", "global", "likes dark mode", 1, "{}");
    insert(&store, "m2", "global", "prefers dark themes", 2, "{}");
    insert(&store, "m3", "global", "going to lunch", 3, "{}");
    insert(&store, "m4", "global", "untouched by the model", 4, "{}");
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES('e1','m1','s','doc','t','t',0,1,'exact','2026-05-01 00:00:00 +0000 UTC')",
            [],
        )
        .unwrap();
    let out = with_engine(vec![Ok(MERGE_1_2.into())], false, |e, seen| {
        let out = store.run_consolidation(e, "", false).unwrap();
        let prompts = seen.borrow();
        assert_eq!(prompts.len(), 1);
        assert!(
            prompts[0].1.contains("- [1] Content: ") && prompts[0].1.contains("- [4] Content: ")
        );
        assert!(!prompts[0].1.contains("m1"), "uuids never reach the model");
        out
    });
    assert_eq!(out[0].new_memories[0].id, "new-1");
    // Unknown index 9/7 dropped; m3 discarded; m4 unmentioned stays raw.
    assert_eq!(out[0].archived_memory_ids, ["m1", "m2", "m3"]);
    let q = |sql: &str, id: &str| cell(&store, sql, id);
    for id in ["m1", "m2"] {
        assert_eq!(
            q("SELECT consolidation_status FROM memories WHERE id=?", id).as_deref(),
            Some("archived")
        );
        assert_eq!(
            q("SELECT consolidated_into_id FROM memories WHERE id=?", id).as_deref(),
            Some("new-1")
        );
    }
    assert_eq!(
        q("SELECT consolidation_status FROM memories WHERE id=?", "m3").as_deref(),
        Some("archived")
    );
    assert_eq!(
        q("SELECT consolidated_into_id FROM memories WHERE id=?", "m3"),
        None
    );
    assert_eq!(
        q("SELECT consolidation_status FROM memories WHERE id=?", "m4").as_deref(),
        Some("raw")
    );
    assert_eq!(
        q("SELECT content FROM memories WHERE id=?", "new-1").as_deref(),
        Some("Daniel prefers dark mode.")
    );
    let meta = q("SELECT metadata FROM memories WHERE id=?", "new-1").unwrap();
    assert!(
        meta.contains("\"source_untrusted\":\"true\"") && meta.contains("\"topic\":\"prefs\""),
        "{meta}"
    );
    assert_eq!(
        q("SELECT memory_id FROM memory_evidence WHERE id=?", "e1").as_deref(),
        Some("new-1")
    );

    // Undo restores the originals, moves evidence back and consumes the run.
    let text = store.undo_last_consolidation().unwrap();
    assert!(
        text.contains("Deleted consolidated memories: 1")
            && text.contains("Re-activated archived originals: 3"),
        "{text}"
    );
    assert!(text.contains("(from 2026-06-01 12:00:00):"), "{text}");
    assert_eq!(q("SELECT 1 FROM memories WHERE id=?", "new-1"), None);
    for id in ["m1", "m2", "m3"] {
        assert_eq!(
            q("SELECT consolidation_status FROM memories WHERE id=?", id).as_deref(),
            Some("raw")
        );
        assert_eq!(
            q("SELECT consolidated_into_id FROM memories WHERE id=?", id),
            None
        );
    }
    assert!(
        q("SELECT memory_id FROM memory_evidence WHERE id=?", "e1")
            .is_some_and(|m| m == "m1" || m == "m2")
    );
    let again = store.undo_last_consolidation().unwrap_err().to_string();
    assert_eq!(again, "no completed consolidation run found to undo");
}

#[test]
fn dry_run_writes_nothing() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "m1", "global", "one fact", 1, "{}");
    insert(&store, "m2", "global", "two fact", 2, "{}");
    let out = with_engine(vec![Ok(MERGE_1_2.into())], false, |e, _| {
        store.run_consolidation(e, "", true).unwrap()
    });
    assert_eq!(out[0].new_memories.len(), 1);
    let n: i64 = store
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(
        cell(
            &store,
            "SELECT consolidation_status FROM memories WHERE id=?",
            "m1"
        )
        .as_deref(),
        Some("raw")
    );
    assert!(store.last_completed_consolidation_run().unwrap().is_none());
}

#[test]
fn failing_scopes_are_skipped_and_all_failing_is_an_error() {
    let store = Store::open_in_memory().unwrap();
    for (id, scope, day) in [
        ("a1", "alpha", 1),
        ("a2", "alpha", 2),
        ("b1", "beta", 1),
        ("b2", "beta", 2),
    ] {
        insert(&store, id, scope, &format!("fact {id}"), day, "{}");
    }
    // alpha fails (unparseable), beta succeeds.
    let ok = r#"{"consolidated":[{"content":"merged beta","replaces_ids":["1","2"]}],"discarded_ids":[]}"#;
    let out = with_engine(vec![Ok("nope".into()), Ok(ok.into())], false, |e, _| {
        store.run_consolidation(e, "", false).unwrap()
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].scope, "beta");
    assert_eq!(
        cell(
            &store,
            "SELECT consolidation_status FROM memories WHERE id=?",
            "a1"
        )
        .as_deref(),
        Some("raw")
    );

    let error = with_engine(vec![Err("boom".into())], false, |e, _| {
        store
            .run_consolidation(e, "alpha", false)
            .unwrap_err()
            .to_string()
    });
    assert_eq!(
        error,
        "consolidation failed: all scopes failed (1): first error for scope \"alpha\": boom"
    );
}

#[test]
fn pii_redaction_scope_filter_and_code_family() {
    let store = Store::open_in_memory().unwrap();
    insert(
        &store,
        "c1",
        "proj",
        "cargo build uses jobs=3",
        1,
        r#"{"prompt_mode":"code"}"#,
    );
    insert(
        &store,
        "c2",
        "proj",
        "cargo test is locked",
        2,
        r#"{"prompt_mode":"code"}"#,
    );
    insert(&store, "o1", "other", "x", 1, "{}");
    insert(&store, "o2", "other", "y", 2, "{}");
    let reply = r#"{"consolidated":[{"content":"Contact max@example.com for builds.","replaces_ids":["1","2"]}],"discarded_ids":[]}"#;
    with_engine(vec![Ok(reply.into())], true, |e, seen| {
        let out = store.run_consolidation(e, "proj", false).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            !out[0].new_memories[0].content.contains("max@example.com"),
            "{}",
            out[0].new_memories[0].content
        );
        let prompts = seen.borrow();
        assert_eq!(prompts.len(), 1, "other scope filtered out");
        assert!(prompts[0].0.contains("(code mode)"));
    });
    assert_eq!(
        cell(
            &store,
            "SELECT consolidation_status FROM memories WHERE id=?",
            "o1"
        )
        .as_deref(),
        Some("raw")
    );
}

#[test]
fn consolidation_for_given_memories_ignores_status() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "w1", "global", "temp a", 1, "{}");
    insert(&store, "w2", "global", "temp b", 2, "{}");
    store
        .lock()
        .unwrap()
        .execute(
            "UPDATE memories SET consolidation_status='archived' WHERE id='w2'",
            [],
        )
        .unwrap();
    let reply = r#"{"consolidated":[{"content":"temp summary","replaces_ids":["1","2"]}],"discarded_ids":[]}"#;
    let out = with_engine(vec![Ok(reply.into())], false, |e, _| {
        store
            .run_consolidation_for_memories(e, &["w1".into(), "w2".into(), "missing".into()], false)
            .unwrap()
    });
    assert_eq!(out[0].archived_memory_ids, ["w1", "w2"]);
    assert!(
        with_engine(vec![], false, |e, _| store
            .run_consolidation_for_memories(e, &[], false)
            .unwrap())
        .is_empty()
    );
}
