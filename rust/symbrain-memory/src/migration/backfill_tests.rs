//! Backfill eligibility must distinguish proven incompleteness from valid NULLs.

use super::{catalog, configure, historical_tests::valid_uuid4, test_support::*};
use crate::schema::{INDEXES, SCHEMA};
use rusqlite::Connection;

fn nullable_rows(conn: &Connection, id: &str) {
    entities(conn);
    conn.execute("INSERT INTO rules(id,content,scope,metadata,created_at,updated_at) VALUES ('rule','rule','global','{}',?,NULL)", [CREATED]).unwrap();
    conn.execute("INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,id,created_at,updated_at) VALUES ('a','b','knows',?,?,NULL)", [id, CREATED]).unwrap();
}

#[test]
fn known_native_false_completion_repairs_rule_and_relation_provenance() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(INDEXES).unwrap();
    for (version, _) in catalog::STEPS {
        conn.execute(
            "INSERT INTO schema_migrations(version) VALUES (?)",
            [version],
        )
        .unwrap();
    }
    nullable_rows(&conn, "");
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
    let (id, updated) = conn
        .query_row("SELECT id,updated_at FROM entity_relations", [], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap();
    assert!(valid_uuid4(&id));
    assert_eq!(updated, CREATED);
    assert_eq!(
        conn.query_row("SELECT updated_at FROM rules", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        CREATED
    );
}

#[test]
fn healthy_claimed_migrations_preserve_unrelated_nullable_rows_and_ids() {
    let conn = Connection::open_in_memory().unwrap();
    prefix(&conn, 36, false);
    nullable_rows(&conn, "caller-kept-id");
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
    assert_eq!(
        scalar(&conn, "SELECT count(*) FROM rules WHERE updated_at IS NULL"),
        1
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM entity_relations WHERE id='caller-kept-id' AND updated_at IS NULL AND valid_from IS NULL AND valid_until IS NULL"
        ),
        1
    );
}

#[test]
fn claimed_blank_identity_repairs_only_its_unchanged_relation_triple() {
    let conn = Connection::open_in_memory().unwrap();
    prefix(&conn, 36, false);
    nullable_rows(&conn, "");
    conn.execute("INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,id,created_at) VALUES ('b','a','other','stable',?)", [CREATED]).unwrap();
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
    assert_eq!(
        scalar(&conn, "SELECT count(*) FROM rules WHERE updated_at IS NULL"),
        1
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM entity_relations WHERE id='stable' AND updated_at IS NULL"
        ),
        1
    );
    let (id, updated) = conn
        .query_row(
            "SELECT id,updated_at FROM entity_relations WHERE from_entity_id='a'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert!(valid_uuid4(&id));
    assert_eq!(updated, CREATED);
}

#[test]
fn missing_claimed_rule_column_is_sufficient_evidence_to_backfill() {
    let conn = Connection::open_in_memory().unwrap();
    let old = SCHEMA.replace("updated_at DATETIME, created_by TEXT NOT NULL DEFAULT '', updated_by TEXT NOT NULL DEFAULT ''", "created_by TEXT NOT NULL DEFAULT '', updated_by TEXT NOT NULL DEFAULT ''");
    assert_ne!(old, SCHEMA);
    conn.execute_batch(&old).unwrap();
    for (version, _) in catalog::STEPS {
        conn.execute(
            "INSERT INTO schema_migrations(version) VALUES (?)",
            [version],
        )
        .unwrap();
    }
    conn.execute("INSERT INTO rules(id,content,scope,metadata,created_at) VALUES ('rule','rule','global','{}',?)", [CREATED]).unwrap();
    let store = configure(conn).unwrap();
    assert_eq!(
        store
            .lock()
            .unwrap()
            .query_row("SELECT updated_at FROM rules", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        CREATED
    );
}

#[test]
fn nullable_attribution_and_target_fields_keep_existing_cells() {
    let conn = Connection::open_in_memory().unwrap();
    prefix(&conn, 28, false);
    conn.execute_batch("INSERT INTO query_log(id,tool,query_text,params,created_at) VALUES ('query','search','kept','{}','2000-01-02 03:04:05'); INSERT INTO audit_log(id,action,detail,created_at) VALUES ('audit','set','kept','2000-01-02 03:04:05');").unwrap();
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM query_log WHERE query_text='kept' AND params='{}' AND actor IS NULL AND scope IS NULL AND session IS NULL"
        ),
        1
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM audit_log WHERE detail='kept' AND target_type IS NULL AND target_id IS NULL"
        ),
        1
    );
}
