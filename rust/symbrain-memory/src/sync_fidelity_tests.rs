//! Full-fidelity sync replica (#855): vectors survive partial payloads,
//! lifecycle fields travel on updates, equal-timestamp lifecycle edits
//! converge, and the entity graph, links and evidence round-trip between two
//! independent stores.

use crate::Store;
use crate::sync_client::{ClientApplyMode, GoTime, apply_body_with_graph, decode_changes};
use rusqlite::params;

const MODE: ClientApplyMode = ClientApplyMode {
    quantize_binary: false,
    audit_enabled: true,
};
const ID: &str = "11111111-1111-4111-8111-111111111111";
const SECOND: &str = "22222222-2222-4222-8222-222222222222";

fn vector(seed: f32) -> String {
    let items: Vec<String> = (0..768)
        .map(|index| {
            format!(
                "{}",
                seed + f32::from(u16::try_from(index).unwrap()) / 1000.0
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

struct Wire<'a> {
    id: &'a str,
    content: &'a str,
    updated: &'a str,
    embedding: Option<String>,
    extra: &'a str,
}

fn apply(store: &Store, wire: &Wire<'_>) -> bool {
    let embedding = wire
        .embedding
        .as_ref()
        .map_or(String::new(), |vector| format!("\"embedding\":{vector},"));
    let body = format!(
        "{{\"memories\":[{{\"id\":\"{}\",\"content\":\"{}\",\"scope\":\"global\",\"metadata\":{{}},{embedding}\"created_at\":\"2026-10-10T08:00:00Z\",\"updated_at\":\"{}\",\"consolidation_status\":\"raw\",\"importance\":0.5,\"review_status\":\"approved\",\"tier\":\"long_term\",\"access_count\":1{}}}],\"deleted\":null}}",
        wire.id, wire.content, wire.updated, wire.extra
    );
    let changes = decode_changes(body.as_bytes(), None).expect("decode");
    let memory = changes.memories[0].as_ref().expect("memory");
    store.client_upsert_if_newer(memory, MODE).expect("apply")
}

fn column(store: &Store, id: &str, name: &str) -> rusqlite::types::Value {
    let conn = store.lock().unwrap();
    conn.query_row(
        &format!("SELECT {name} FROM memories WHERE id=?"),
        [id],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn partial_payload_keeps_a_valid_stored_embedding() {
    let store = Store::open_in_memory().unwrap();
    let full = Wire {
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: Some(vector(0.1)),
        extra: "",
    };
    assert!(apply(&store, &full));
    let before = column(&store, ID, "embedding");
    let lite = Wire {
        updated: "2026-10-12T08:00:00Z",
        embedding: None,
        extra: ",\"kind\":\"note\"",
        ..full
    };
    assert!(apply(&store, &lite));
    assert_eq!(column(&store, ID, "embedding"), before);
    assert_eq!(
        column(&store, ID, "embedding_dim"),
        rusqlite::types::Value::Integer(768)
    );
    assert_eq!(
        column(&store, ID, "kind"),
        rusqlite::types::Value::Text("note".into())
    );
}

#[test]
fn content_change_without_a_vector_clears_the_stale_one() {
    let store = Store::open_in_memory().unwrap();
    let first = Wire {
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: Some(vector(0.1)),
        extra: "",
    };
    assert!(apply(&store, &first));
    let changed = Wire {
        content: "beta",
        updated: "2026-10-12T08:00:00Z",
        embedding: None,
        ..first
    };
    assert!(apply(&store, &changed));
    assert_eq!(
        column(&store, ID, "embedding_dim"),
        rusqlite::types::Value::Integer(0)
    );
}

#[test]
fn a_new_vector_replaces_the_stored_one() {
    let store = Store::open_in_memory().unwrap();
    let first = Wire {
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: Some(vector(0.1)),
        extra: "",
    };
    assert!(apply(&store, &first));
    let before = column(&store, ID, "embedding");
    let next = Wire {
        updated: "2026-10-12T08:00:00Z",
        embedding: Some(vector(0.7)),
        ..first
    };
    assert!(apply(&store, &next));
    assert_ne!(column(&store, ID, "embedding"), before);
}

#[test]
fn updates_carry_validity_and_supersession() {
    let store = Store::open_in_memory().unwrap();
    let first = Wire {
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: None,
        extra: "",
    };
    assert!(apply(&store, &first));
    let next = Wire {
        updated: "2026-10-12T08:00:00Z",
        extra: ",\"valid_from\":\"2026-10-01T00:00:00Z\",\"valid_to\":\"2026-10-20T00:00:00Z\",\"superseded_by\":\"22222222-2222-4222-8222-222222222222\",\"retired_at\":\"2026-10-12T07:00:00Z\"",
        ..first
    };
    assert!(apply(&store, &next));
    for name in ["valid_from", "valid_to", "superseded_by", "retired_at"] {
        assert_ne!(
            column(&store, ID, name),
            rusqlite::types::Value::Null,
            "{name}"
        );
    }
}

#[test]
fn equal_timestamp_lifecycle_edits_converge_and_are_audited() {
    let (left, right) = (
        Store::open_in_memory().unwrap(),
        Store::open_in_memory().unwrap(),
    );
    let retired = Wire {
        extra: ",\"retired_at\":\"2026-10-11T07:00:00Z\"",
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: None,
    };
    let superseded = Wire {
        extra: ",\"superseded_by\":\"22222222-2222-4222-8222-222222222222\"",
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: None,
    };
    // `left` holds the retirement, `right` the supersession; same instant.
    assert!(apply(&left, &retired));
    assert!(apply(&right, &superseded));
    let a = apply(&left, &superseded);
    let b = apply(&right, &retired);
    // Exactly one side moves to the greater key, the other keeps its row.
    assert!(a || b);
    for name in ["retired_at", "superseded_by"] {
        assert_eq!(column(&left, ID, name), column(&right, ID, name), "{name}");
    }
    let audited: i64 = [&left, &right]
        .iter()
        .map(|store| {
            store
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM audit_log WHERE action='sync_lifecycle'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        })
        .sum();
    assert_eq!(audited, 1);
}

fn seed_graph(store: &Store) {
    let conn = store.lock().unwrap();
    for (id, name, kind, at) in [
        (
            "e-alice",
            "Alice",
            "person",
            "2026-10-02 08:00:00 +0000 UTC",
        ),
        ("e-bob", "Bob", "person", "2026-10-02 08:00:00 +0000 UTC"),
        ("e-lone", "Lonely", "other", "2026-10-03 08:00:00 +0000 UTC"),
    ] {
        conn.execute(
            "INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
            params![id, name, kind, "[\"al\"]", format!("{name} desc"), "tester", at, at],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_by,created_at,id,source,source_ref,verification,evidence,updated_at,valid_from,valid_until) VALUES('e-alice','e-bob','knows','tester','2026-10-02 08:00:00 +0000 UTC','r-1','import','ref-1','verified','{\"k\":1}','2026-10-04 08:00:00 +0000 UTC','2026-01-01 00:00:00 +0000 UTC','2027-01-01 00:00:00 +0000 UTC')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_entities(memory_id,entity_id) VALUES(?, 'e-alice')",
        [ID],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES('ev-1',?,'src','doc','alpha text','alpha',0,5,'exact','2026-10-05 08:00:00 +0000 UTC')",
        [ID],
    )
    .unwrap();
}

fn graph_dump(store: &Store) -> Vec<String> {
    let conn = store.lock().unwrap();
    let mut out = Vec::new();
    for query in [
        "SELECT 'entity', name, type, aliases, description, created_by FROM entities ORDER BY name",
        "SELECT 'link', me.memory_id, e.name, '', '', '' FROM memory_entities me JOIN entities e ON e.id=me.entity_id ORDER BY 2,3",
        "SELECT 'relation', e1.name, e2.name, r.relation_type, r.source || '/' || r.source_ref || '/' || r.verification || '/' || r.evidence, COALESCE(r.valid_from,'') || '|' || COALESCE(r.valid_until,'') FROM entity_relations r JOIN entities e1 ON e1.id=r.from_entity_id JOIN entities e2 ON e2.id=r.to_entity_id ORDER BY 2,3,4",
        "SELECT 'evidence', id, memory_id, source_id || source_kind || text || evidence_text, char_start || '-' || char_end, alignment_status FROM memory_evidence ORDER BY id",
    ] {
        let mut statement = conn.prepare(query).unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((0..6)
                    .map(|index| {
                        format!("{:?}", row.get::<_, rusqlite::types::Value>(index).unwrap())
                    })
                    .collect::<Vec<_>>()
                    .join(","))
            })
            .unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

#[test]
fn entities_relations_links_and_evidence_round_trip_between_stores() {
    let source = Store::open_in_memory().unwrap();
    let plain = Wire {
        id: ID,
        content: "alpha",
        updated: "2026-10-11T08:00:00Z",
        embedding: Some(vector(0.3)),
        extra: "",
    };
    assert!(apply(&source, &plain));
    seed_graph(&source);

    // What the push puts on the wire, decoded as the receiving peer would.
    let memories = source
        .client_local_memories(&GoTime::zero(), "", 200)
        .unwrap();
    assert_eq!(memories[0].embedding.len(), 768);
    assert_eq!(memories[0].extras.entities, ["Alice"]);
    assert_eq!(memories[0].extras.evidence.len(), 1);
    let graph = source.client_graph_since(&GoTime::zero()).unwrap();
    assert_eq!(graph.entities.len(), 3);
    assert_eq!(graph.relations.len(), 1);
    let body = apply_body_with_graph(&memories, &[], &graph);
    let changes = decode_changes(body.as_bytes(), None).unwrap();
    assert_eq!(changes.graph, graph);

    let replica = Store::open_in_memory().unwrap();
    replica.client_apply_graph(&changes.graph).unwrap();
    let memory = changes.memories[0].as_ref().unwrap();
    assert!(replica.client_upsert_if_newer(memory, MODE).unwrap());
    replica
        .client_apply_extras(&memory.id, &memory.extras)
        .unwrap();

    assert_eq!(graph_dump(&source), graph_dump(&replica));
    assert!(
        graph_dump(&replica)
            .iter()
            .any(|row| row.contains("Lonely"))
    );
    assert_eq!(
        column(&replica, ID, "embedding"),
        column(&source, ID, "embedding")
    );
    // Idempotent: a second application changes nothing.
    replica.client_apply_graph(&changes.graph).unwrap();
    replica
        .client_apply_extras(&memory.id, &memory.extras)
        .unwrap();
    assert_eq!(graph_dump(&source), graph_dump(&replica));
    let _ = SECOND;
}

#[test]
fn entities_merge_by_name_when_ids_differ() {
    let source = Store::open_in_memory().unwrap();
    seed_graph_entities_only(&source, "src-alice");
    let replica = Store::open_in_memory().unwrap();
    seed_graph_entities_only(&replica, "local-alice");
    let graph = source.client_graph_since(&GoTime::zero()).unwrap();
    replica.client_apply_graph(&graph).unwrap();
    let conn = replica.lock().unwrap();
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM entities WHERE name='Alice' COLLATE NOCASE")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, ["local-alice"], "the local id survives, no duplicate");
}

fn seed_graph_entities_only(store: &Store, id: &str) {
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?, 'Alice', 'person', '[]', '', 't', '2026-10-02 08:00:00 +0000 UTC', '2026-10-02 08:00:00 +0000 UTC')",
            [id],
        )
        .unwrap();
}
