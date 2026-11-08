//! #855: `memory sync` is a full-fidelity replica. Two independent stores,
//! one served by `memory serve`, converge through the plain exchange and the
//! encrypted relay on every supported datum: content, provenance,
//! lifecycle/retirement/supersession, vectors, entities, relations, links and
//! evidence. Comparison covers all supported columns, not ids or counts.
//! Dummy databases in disposable HOME/XDG roots only.
#![allow(clippy::pedantic)]

mod memory_serve_support;

use memory_serve_support::*;
use rusqlite::{Connection, params};
use std::path::Path;

const M1: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const M2: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn vector(seed: f32) -> String {
    let items: Vec<String> = (0..768)
        .map(|index| format!("{}", seed + index as f32 / 1000.0))
        .collect();
    format!("[{}]", items.join(","))
}

fn create(path: &Path) -> Connection {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    drop(symbrain_memory::Store::open(path).expect("store"));
    Connection::open(path).unwrap()
}

fn memory(conn: &Connection, id: &str, content: &str, seed: f32, lifecycle: (&str, &str, &str)) {
    conn.execute(
        "INSERT INTO memories (id, content, scope, metadata, embedding, embedding_dim, embedding_source, embedding_model, created_by, created_session, created_at, updated_at, kind, valid_from, valid_to, superseded_by, retired_at, importance) VALUES (?, ?, 'global', '{}', ?, 768, 'ollama', 'nomic', 'alice', 's1', '2026-10-10 08:00:00 +0000 UTC', '2026-10-11 08:00:00 +0000 UTC', 'note', ?, ?, NULLIF(?, ''), '2026-10-12 08:00:00 +0000 UTC', 0.8)",
        params![id, content, vector(seed), lifecycle.0, lifecycle.1, lifecycle.2],
    )
    .unwrap();
}

fn entity(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
        params![id, name, "person", "[\"nick\"]", format!("{name} desc"), "tester", "2026-10-02 08:00:00 +0000 UTC", "2026-10-02 08:00:00 +0000 UTC"],
    )
    .unwrap();
}

fn graph(conn: &Connection, memory_id: &str, from: &str, to: &str, tag: &str) {
    let (from_id, to_id) = (format!("e-{from}"), format!("e-{to}"));
    entity(conn, &from_id, from);
    entity(conn, &to_id, to);
    conn.execute(
        "INSERT INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
        params![memory_id, from_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_by,created_at,id,source,source_ref,verification,evidence,updated_at,valid_from,valid_until) VALUES(?,?,'knows','tester','2026-10-02 08:00:00 +0000 UTC',?,'import','ref','verified','{\"k\":1}','2026-10-04 08:00:00 +0000 UTC','2026-01-01 00:00:00 +0000 UTC','2027-01-01 00:00:00 +0000 UTC')",
        params![from_id, to_id, format!("rel-{tag}")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES(?,?,'src','doc','full text','full',0,4,'exact','2026-10-05 08:00:00 +0000 UTC')",
        params![format!("ev-{tag}"), memory_id],
    )
    .unwrap();
}

fn dump(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut out = Vec::new();
    for query in [
        "SELECT 'memory', id, content, scope, metadata, embedding, embedding_dim, embedding_source, embedding_model, created_by, created_session, consolidation_status, COALESCE(consolidated_into_id,''), importance, COALESCE(valid_from,''), COALESCE(valid_to,''), COALESCE(superseded_by,''), kind, review_status, decay_factor, COALESCE(retired_at,''), tier FROM memories ORDER BY id",
        "SELECT 'entity', name, type, aliases, description, created_by, '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '' FROM entities ORDER BY name",
        "SELECT 'link', me.memory_id, e.name, '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '' FROM memory_entities me JOIN entities e ON e.id=me.entity_id ORDER BY 2,3",
        "SELECT 'relation', e1.name, e2.name, r.relation_type, r.source, r.source_ref, r.verification, r.evidence, COALESCE(r.valid_from,''), COALESCE(r.valid_until,''), '', '', '', '', '', '', '', '', '', '', '', '' FROM entity_relations r JOIN entities e1 ON e1.id=r.from_entity_id JOIN entities e2 ON e2.id=r.to_entity_id ORDER BY 2,3,4",
        "SELECT 'evidence', id, memory_id, source_id, source_kind, text, evidence_text, char_start, char_end, alignment_status, '', '', '', '', '', '', '', '', '', '', '', '' FROM memory_evidence ORDER BY id",
    ] {
        let mut statement = conn.prepare(query).unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((0..22)
                    .map(|index| {
                        format!("{:?}", row.get::<_, rusqlite::types::Value>(index).unwrap())
                    })
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

fn sync(root: &Root, remote: &str, extra: &[&str]) {
    let bearer = token("writer", "jti-fidelity");
    let mut args = vec![
        "sync",
        "--remote",
        remote,
        "--token",
        bearer.as_str(),
        "--json",
    ];
    args.extend_from_slice(extra);
    let output = root.command(&rust_binary(), &args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seeded_pair() -> (Root, Root) {
    let (server, client) = (Root::new(), Root::new());
    let a = create(&server.db());
    memory(
        &a,
        M2,
        "server memory",
        0.2,
        (
            "2026-09-01 00:00:00 +0000 UTC",
            "2026-12-01 00:00:00 +0000 UTC",
            M1,
        ),
    );
    graph(&a, M2, "Carol", "Dave", "a");
    entity(&a, "e-lonely", "Lonely");
    let b = create(&client.db());
    memory(
        &b,
        M1,
        "client memory",
        0.6,
        (
            "2026-08-01 00:00:00 +0000 UTC",
            "2026-11-01 00:00:00 +0000 UTC",
            "",
        ),
    );
    graph(&b, M1, "Alice", "Bob", "b");
    (server, client)
}

#[test]
fn plain_sync_converges_both_stores_on_every_supported_datum() {
    let (server, client) = seeded_pair();
    let running = Server::start(&server, &rust_binary(), &[], &[]);
    let remote = format!("http://127.0.0.1:{}", running.port);
    sync(&client, &remote, &[]);
    let _ = running.terminate();
    let (left, right) = (dump(&server.db()), dump(&client.db()));
    assert_eq!(left, right);
    let text = left.join("\n");
    for needle in [
        "client memory",
        "server memory",
        "Carol",
        "Dave",
        "Lonely",
        "Alice",
        "ev-a",
        "ev-b",
        "rel",
        "verified",
    ] {
        assert!(text.contains(needle), "{needle} missing:\n{text}");
    }
    // Vectors, supersession and retirement survived on both sides.
    assert!(
        left.iter()
            .filter(|row| row.starts_with("Text(\"memory\")"))
            .all(|row| row.contains("Integer(768)"))
    );
}

#[test]
fn relay_sync_replicates_into_an_empty_store_without_loss() {
    let (server, client) = seeded_pair();
    let running = Server::start(&server, &rust_binary(), &[], &[]);
    let remote = format!("http://127.0.0.1:{}", running.port);
    let relay = [
        "--encrypted-relay",
        "--relay-passphrase",
        "correct horse battery",
    ];
    // The client publishes its store; a fresh peer pulls it through the relay.
    sync(&client, &remote, &relay);
    let fresh = Root::new();
    drop(create(&fresh.db()));
    sync(
        &fresh,
        &remote,
        &[
            "--pull",
            "--encrypted-relay",
            "--relay-passphrase",
            "correct horse battery",
        ],
    );
    let _ = running.terminate();
    assert_eq!(dump(&client.db()), dump(&fresh.db()));
}
