//! SOURCE_ONLY regression preparation; execute after the scheduler grants a gate.

use super::*;
use crate::{DirectWrite, EmbeddingGenerator};
use std::collections::BTreeMap;

fn request(content: &str) -> DirectWrite {
    DirectWrite {
        content: content.into(),
        scope: "global".into(),
        kind: "reference".into(),
        metadata: BTreeMap::new(),
        author: "writer".into(),
        entities: vec![],
        staged: false,
        quantize_binary: false,
        conflict_enabled: true,
    }
}

fn generator() -> EmbeddingGenerator {
    // An invalid URL is rejected before a client can open an endpoint.
    EmbeddingGenerator::new("owned-invalid-endpoint", "owned-model")
}

fn logged(store: &Store, input: &DirectWrite) -> (Result<String, StoreError>, Vec<String>) {
    let mut logs = Vec::new();
    let result = store.set_direct_cli_with_conflict(input, &generator(), 0.8, 0.95, 10,
        &mut |message, attributes| logs.push(format!("{message} {}", attributes.iter()
            .map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(" "))));
    (result, logs)
}

fn seed(store: &Store, content: &str) -> String {
    let mut input = request(content);
    input.conflict_enabled = false;
    input.author = "original".into();
    let id = store.set_direct_cli(&input, &generator()).unwrap();
    store.lock().unwrap().execute(
        "UPDATE memories SET created_at='2000-01-01 00:00:00 +0000 UTC',updated_at='2000-01-01 00:00:00 +0000 UTC' WHERE id=?", [&id],
    ).unwrap();
    id
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn force_recall(store: &Store, id: &str, query: &str) {
    let hash = crate::lsh::compute_lsh(&crate::embedding::local_hash_vector(query)).unwrap();
    store.lock().unwrap().execute("UPDATE memories SET lsh_hash=? WHERE id=?", params![hash, id]).unwrap();
}

#[test]
fn exact_repeat_keeps_identity_provenance_and_access_but_applies_governance_and_entities() {
    let store = Store::open_in_memory().unwrap();
    let old = seed(&store, "hello world");
    let mut input = request("hello world");
    input.kind = "user".into();
    input.metadata.insert("confidence".into(), "low".into());
    input.entities = vec!["New Entity".into()];
    let (result, logs) = logged(&store, &input);
    let id = result.unwrap();
    assert_eq!(id, old);
    assert_eq!(logs, [format!("INFO conflict: duplicate write deduplicated id={old}")]);
    let conn = store.lock().unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM memories"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='set'"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM memory_entities"), 1);
    let (kind, actor, access, metadata): (String, String, i64, String) = conn.query_row(
        "SELECT kind,created_by,access_count,metadata FROM memories WHERE id=?", [&id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!((kind.as_str(), actor.as_str(), access), ("user", "original", 1));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["confidence"], "high");
}

#[test]
fn near_duplicate_replaces_old_row_after_save_and_closes_at_winner_creation() {
    let store = Store::open_in_memory().unwrap();
    let old = seed(&store, "hello world");
    let id = store.set_direct_cli(&request("world hello"), &generator()).unwrap();
    assert_ne!(id, old);
    let conn = store.lock().unwrap();
    let state: (String, String, String, Option<String>, i64) = conn.query_row(
        "SELECT superseded_by,valid_to,updated_by,updated_session,access_count FROM memories WHERE id=?", [&old],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).unwrap();
    let created: String = conn.query_row("SELECT created_at FROM memories WHERE id=?", [&id], |row| row.get(0)).unwrap();
    assert_eq!(state, (id.clone(), created, "writer".into(), None, 2));
    let detail: String = conn.query_row("SELECT detail FROM audit_log WHERE action='supersede'", [], |row| row.get(0)).unwrap();
    assert_eq!(detail, format!("{{\"winner_id\":\"{id}\",\"winner_actor\":\"writer\",\"loser_actor\":\"original\",\"rule\":\"near_dup\",\"similarity\":1}}"));
}

#[test]
fn ambiguity_limit_and_float32_threshold_are_not_ignored() {
    let store = Store::open_in_memory().unwrap();
    let band = seed(&store, "hello world hello");
    let near = seed(&store, "world hello");
    force_recall(&store, &band, "hello world");
    store.lock().unwrap().execute("UPDATE memories SET importance=10 WHERE id=?", [&band]).unwrap();
    let input = request("hello world");
    let id = store.set_direct_cli_with_conflict(&input, &generator(), 0.8, 0.95, 1, &mut |_, _| {}).unwrap();
    let conn = store.lock().unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='conflict_pending'"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='supersede'"), 0);
    let detail: String = conn.query_row("SELECT detail FROM audit_log WHERE action='conflict_pending' AND memory_id=?", [&id], |row| row.get(0)).unwrap();
    assert!(detail.contains(&band));
    assert!(!detail.contains(&near));
    drop(conn);

    // f64 slightly above the f32 cosine must narrow before comparison.
    let left = crate::embedding::local_hash_vector("hello world");
    let right = crate::embedding::local_hash_vector("hello world hello");
    let cosine = crate::retrieval::cosine_similarity(&left, &right);
    let config = Config { contradiction: 0.8, near_dup: f64::from(cosine) + 1e-10, max_candidates: 1 };
    assert_eq!(config.thresholds().1, cosine);
    let threshold_store = Store::open_in_memory().unwrap();
    let candidate = seed(&threshold_store, "hello world hello");
    force_recall(&threshold_store, &candidate, "hello world");
    let conn = threshold_store.lock().unwrap();
    assert!(matches!(check(&conn, config, "new", "hello world", "global", &GeneratedEmbedding {
        vector: left, source: "hash-fallback".into(),
    }).unwrap(), Plan::NearDup(_)));
}

#[test]
fn staged_disabled_other_scope_and_ineligible_candidates_do_not_supersede() {
    for mode in ["staged", "disabled", "other-scope", "archived", "retired", "candidate", "superseded", "other-source", "other-dimension", "quantized"] {
        let store = Store::open_in_memory().unwrap();
        let old = seed(&store, "hello world");
        let mut input = request("world hello");
        match mode {
            "staged" => input.staged = true,
            "disabled" => input.conflict_enabled = false,
            "other-scope" => input.scope = "user".into(),
            _ => {
                let assignment = match mode {
                    "archived" => "consolidation_status='archived'",
                    "retired" => "retired_at='2000-01-01 00:00:00 +0000 UTC'",
                    "candidate" => "review_status='staged'",
                    "superseded" => "superseded_by='previous'",
                    "other-source" => "embedding_source='ollama'",
                    "other-dimension" => "embedding_dim=3",
                    "quantized" => "embedding_quantization='binary'",
                    _ => unreachable!(),
                };
                store.lock().unwrap().execute(&format!("UPDATE memories SET {assignment} WHERE id=?"), [&old]).unwrap();
            }
        }
        let id = store.set_direct_cli(&input, &generator()).unwrap();
        assert_ne!(id, old, "{mode}");
        let conn = store.lock().unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action IN ('supersede','conflict_pending')"), 0, "{mode}");
    }
}

#[test]
fn hash_repeat_has_no_ttl_or_embedding_space_filter() {
    let store = Store::open_in_memory().unwrap();
    let old = seed(&store, "hello world");
    store.lock().unwrap().execute(
        "UPDATE memories SET tier='working',expires_at='2000-01-01 00:00:00 +0000 UTC',embedding_source='ollama',embedding='null',embedding_dim=0 WHERE id=?", [&old],
    ).unwrap();
    assert_eq!(store.set_direct_cli(&request("hello world"), &generator()).unwrap(), old);
}

#[test]
fn checker_errors_degrade_without_accepting_malformed_hydration() {
    for (field, value, detail) in [
        ("metadata", "bad json", "invalid character 'b' looking for beginning of value"),
        ("metadata", "{\"x\":1,\"x\":\"valid\"}", "json: cannot unmarshal number into Go value of type string"),
        ("embedding", "[1e39]", "json: cannot unmarshal number 1e39 into Go value of type float32"),
        ("created_at", "2000-01-01 00:00:60 +0000 UTC", "sql: Scan error on column index 9, name \"created_at\": unsupported Scan, storing driver.Value type string into type *time.Time"),
        ("updated_at", "invalid", "sql: Scan error on column index 10, name \"updated_at\": unsupported Scan, storing driver.Value type string into type *time.Time"),
    ] {
        let store = Store::open_in_memory().unwrap();
        let old = seed(&store, "hello world");
        store.lock().unwrap().execute(&format!("UPDATE memories SET {field}=? WHERE id=?"), params![value, old]).unwrap();
        let conn = store.lock().unwrap();
        assert!(admission(&conn, "global").is_err());
        drop(conn);
        // Direct service invocation exercises degradation independently of
        // the CLI's read-only Go-routing boundary.
        let (result, logs) = logged(&store, &request("hello world"));
        let id = result.unwrap();
        assert_eq!(logs, [format!("WARN conflict check degraded; storing memory unchanged error=conflict: duplicate lookup: {detail}")]);
        assert_ne!(id, old, "{field}");
        let conn = store.lock().unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM memories"), 2);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='supersede'"), 0);
    }
}

#[test]
fn feedback_and_supersession_callbacks_keep_committed_steps() {
    for (trigger, audit_count, old_exists, superseded) in [
        ("CREATE TRIGGER fixture BEFORE UPDATE OF access_count ON memories BEGIN SELECT RAISE(ABORT,'feedback denied'); END", 1, 1, true),
        ("CREATE TRIGGER fixture BEFORE UPDATE OF superseded_by ON memories BEGIN SELECT RAISE(ABORT,'supersession denied'); END", 0, 1, false),
        ("CREATE TRIGGER fixture BEFORE UPDATE OF superseded_by ON memories BEGIN SELECT RAISE(IGNORE); END", 1, 1, false),
        ("CREATE TRIGGER fixture AFTER UPDATE OF access_count ON memories BEGIN DELETE FROM memories WHERE id=old.id; END", 1, 0, false),
    ] {
        let store = Store::open_in_memory().unwrap();
        let old = seed(&store, "hello world");
        store.lock().unwrap().execute_batch(trigger).unwrap();
        let (result, logs) = logged(&store, &request("world hello"));
        let id = result.unwrap();
        if trigger.contains("supersession denied") {
            assert_eq!(logs, [format!("WARN failed to mark memory superseded loser={old} error=constraint failed: supersession denied (1811)")]);
        } else {
            assert!(logs.is_empty(), "retrieval feedback remains silent, like Go");
        }
        let conn = store.lock().unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='supersede'"), audit_count);
        let existing: i64 = conn.query_row("SELECT COUNT(*) FROM memories WHERE id=?", [&old], |row| row.get(0)).unwrap();
        assert_eq!(existing, old_exists);
        if superseded {
            let winner: String = conn.query_row("SELECT superseded_by FROM memories WHERE id=?", [&old], |row| row.get(0)).unwrap();
            assert_eq!(winner, id);
        }
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM memories WHERE content='world hello'"), 1);
    }
}

#[test]
fn repeat_governance_failure_does_not_create_a_new_row_or_set_audit() {
    let store = Store::open_in_memory().unwrap();
    let old = seed(&store, "hello world");
    store.lock().unwrap().execute_batch("CREATE TRIGGER fixture BEFORE UPDATE OF kind ON memories BEGIN SELECT RAISE(IGNORE); END").unwrap();
    let (result, logs) = logged(&store, &request("hello world"));
    let error = result.unwrap_err();
    assert_eq!(logs, [format!("INFO conflict: duplicate write deduplicated id={old}")]);
    assert_eq!(error.to_string(), format!("memory not found: {old}"));
    let conn = store.lock().unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM memories"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action='set'"), 1);
}

#[test]
fn service_boundary_does_not_inherit_cli_conflict_support() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.set_direct_service(&request("hello world"), &generator(), "http", None, true).is_err());
    let mut input = request("hello world");
    input.conflict_enabled = false;
    let first = store.set_direct_service(&input, &generator(), "http", None, false).unwrap();
    let second = store.set_direct_service(&input, &generator(), "http", None, false).unwrap();
    assert_ne!(first, second);
    assert_eq!(count(&store.lock().unwrap(), "SELECT COUNT(*) FROM audit_log"), 0);
}

#[test]
fn capped_recall_keeps_f64_importance_until_the_final_score() {
    let vector = crate::embedding::local_hash_vector("world hello");
    let cosine = crate::retrieval::cosine_similarity(&vector, &vector);
    let midpoint = (f64::from(0.7_f32) + f64::from(f32::from_bits(0.7_f32.to_bits() + 1))) / 2.0;
    let center = (midpoint - 0.6 * f64::from(cosine)) * 50.0;
    for (low, high) in [(center - 1e-8, center + 1e-8), (4e38, 8e38)] {
        let store = Store::open_in_memory().unwrap();
        let lower = seed(&store, "hello world");
        let higher = seed(&store, "hello world alternate");
        {
            let conn = store.lock().unwrap();
            for (id, importance, created) in [(&lower, low, "2001-01-01"), (&higher, high, "2000-01-01")] {
                conn.execute("UPDATE memories SET importance=?,embedding=?,lsh_hash=?,created_at=? WHERE id=?",
                    params![importance, embedding_json(&vector), crate::lsh::compute_lsh(&vector).unwrap(), format!("{created} 00:00:00 +0000 UTC"), id]).unwrap();
                let hydrated = conn.query_row(&format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE id=?"),
                    [id], SearchRow::from_conflict_row).unwrap();
                assert_eq!(hydrated.importance.to_bits(), importance.to_bits());
            }
        }
        let id = store.set_direct_cli_with_conflict(&request("world hello"), &generator(), 0.8, 0.95, 1, &mut |_, _| {}).unwrap();
        let conn = store.lock().unwrap();
        let state: (String, i64) = conn.query_row("SELECT superseded_by,access_count FROM memories WHERE id=?", [&higher], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(state, (id, 2));
        let state: (Option<String>, i64) = conn.query_row("SELECT superseded_by,access_count FROM memories WHERE id=?", [&lower], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(state, (None, 1));
    }
}

#[test]
fn conflict_audit_failures_emit_their_real_sqlite_error_and_keep_writes() {
    for (action, content) in [("conflict_pending", "hello world hello"), ("supersede", "world hello")] {
        let store = Store::open_in_memory().unwrap();
        let old = seed(&store, "hello world");
        force_recall(&store, &old, content);
        store.lock().unwrap().execute_batch(&format!("CREATE TRIGGER fixture BEFORE INSERT ON audit_log WHEN new.action='{action}' BEGIN SELECT RAISE(ABORT,'audit denied'); END")).unwrap();
        let (result, logs) = logged(&store, &request(content));
        let id = result.unwrap();
        assert_ne!(id, old);
        assert_eq!(logs, [format!("WARN failed to write {action} audit event error=constraint failed: audit denied (1811)")]);
        assert_eq!(count(&store.lock().unwrap(), "SELECT COUNT(*) FROM memories"), 2);
    }
}

#[test]
fn sql_scan_errors_precede_metadata_json_in_recall_and_full_hydration() {
    for (query, field, index, stage) in [
        ("hello world", "created_at", 9, "duplicate lookup"),
        ("hello world", "updated_at", 10, "duplicate lookup"),
        ("world hello", "created_at", 4, "candidate recall"),
    ] {
        for metadata in ["{}", "bad json"] {
            let store = Store::open_in_memory().unwrap();
            let old = seed(&store, "hello world");
            force_recall(&store, &old, query);
            assert!(admission(&store.lock().unwrap(), "global").is_ok());
            store.lock().unwrap().execute(
                &format!("UPDATE memories SET {field}='invalid time',metadata=? WHERE id=?"),
                params![metadata, old],
            ).unwrap();
            let (result, logs) = logged(&store, &request(query));
            assert_ne!(result.unwrap(), old);
            let detail = format!("sql: Scan error on column index {index}, name \"{field}\": unsupported Scan, storing driver.Value type string into type *time.Time");
            assert_eq!(logs, [format!("WARN conflict check degraded; storing memory unchanged error=conflict: {stage}: {detail}")]);
            let conn = store.lock().unwrap();
            assert_eq!(count(&conn, "SELECT COUNT(*) FROM memories"), 2);
            assert_eq!(count(&conn, "SELECT COUNT(*) FROM audit_log WHERE action IN ('supersede','conflict_pending')"), 0);
        }
    }
}
