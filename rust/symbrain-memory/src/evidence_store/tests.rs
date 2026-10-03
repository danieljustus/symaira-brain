use super::*;
use crate::evidence::{SourceRef, Span};

fn extraction(status: &str) -> Extraction {
    Extraction {
        source: SourceRef {
            id: "source-1".into(),
            kind: "activity_segment".into(),
        },
        text: "prefers dark mode".into(),
        evidence_text: "dark mode".into(),
        span: Span { start: 5, end: 14 },
        alignment_status: status.into(),
        ..Extraction::default()
    }
}

fn memory(store: &Store) -> String {
    store
        .set(
            "prefers dark mode",
            "global",
            "user",
            serde_json::Map::new(),
            false,
        )
        .unwrap()
        .id
}

#[test]
fn grounded_only_persistence_and_cascade_match_go() {
    let store = Store::open_in_memory().unwrap();
    let id = memory(&store);
    let mut empty = extraction("exact");
    empty.evidence_text.clear();
    let mut invalid = extraction("exact");
    invalid.span.start = -1;
    store
        .save_memory_evidence(
            &id,
            &[
                extraction("exact"),
                extraction("normalized"),
                extraction("fuzzy"),
                extraction("unmatched"),
                empty,
                invalid,
            ],
        )
        .unwrap();
    let rows = store.get_memory_evidence(&id).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].source_id, "source-1");
    assert_eq!(rows[0].alignment_status, "exact");
    assert_eq!(rows[1].alignment_status, "normalized");
    assert_eq!(rows[0].created_at, rows[1].created_at);
    assert_eq!(rows[0].id.len(), 36);
    assert_ne!(rows[0].id, rows[1].id);
    store.delete(&id).unwrap();
    assert!(store.get_memory_evidence(&id).unwrap().is_empty());
}

#[test]
fn rollback_and_reparent_keep_memory_and_evidence_atomic() {
    let store = Store::open_in_memory().unwrap();
    let old = memory(&store);
    let new = memory(&store);
    {
        let mut conn = store.lock().unwrap();
        let tx = conn.transaction().unwrap();
        save_memory_evidence_tx(&tx, &old, &[extraction("exact")]).unwrap();
        tx.rollback().unwrap();
    }
    assert!(store.get_memory_evidence(&old).unwrap().is_empty());
    store
        .save_memory_evidence(&old, &[extraction("exact")])
        .unwrap();
    {
        let mut conn = store.lock().unwrap();
        let tx = conn.transaction().unwrap();
        reparent_memory_evidence_tx(&tx, &old, &new).unwrap();
        reparent_memory_evidence_tx(&tx, "", &new).unwrap();
        reparent_memory_evidence_tx(&tx, &new, &new).unwrap();
        tx.commit().unwrap();
    }
    store.delete(&old).unwrap();
    assert!(store.get_memory_evidence(&old).unwrap().is_empty());
    assert_eq!(store.get_memory_evidence(&new).unwrap().len(), 1);
}

#[test]
fn missing_memory_rejects_grounded_evidence_and_skips_invalid() {
    let store = Store::open_in_memory().unwrap();
    assert!(
        store
            .save_memory_evidence("missing", &[extraction("exact")])
            .is_err()
    );
    store
        .save_memory_evidence("missing", &[extraction("unmatched")])
        .unwrap();
}
