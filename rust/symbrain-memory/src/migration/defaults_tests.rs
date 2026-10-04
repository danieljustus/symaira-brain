//! Public admission and rollback of incompatible omission contracts.

use super::test_support::*;
use crate::schema::{INDEXES, SCHEMA};
use rusqlite::Connection;

fn changed_table(schema: &str, table: &str, old: &str, new: &str) -> String {
    let start = schema
        .find(&format!("CREATE TABLE IF NOT EXISTS {table} ("))
        .expect("owned table");
    let end = start + schema[start..].find(';').expect("owned table terminator") + 1;
    let declaration = &schema[start..end];
    assert_eq!(declaration.matches(old).count(), 1, "{table}.{old}");
    format!(
        "{}{}{}",
        &schema[..start],
        declaration.replacen(old, new, 1),
        &schema[end..]
    )
}

#[test]
fn missing_or_wrong_oplog_default_rejects_before_markers_and_row_repairs() {
    for default in ["", " DEFAULT 'not-a-timestamp'"] {
        let database = OwnedDatabase::new();
        let before = {
            let conn = Connection::open(database.path()).unwrap();
            prefix(&conn, 20, true);
            let resource = super::catalog::STEPS
                .iter()
                .find(|(name, _)| *name == "023_sync_oplog")
                .unwrap()
                .1;
            let invalid =
                resource.replace(" DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))", default);
            assert_ne!(invalid, resource);
            conn.execute_batch(&invalid).unwrap();
            conn.execute_batch("CREATE TRIGGER reject_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(ABORT,'marker must not be reached'); END;").unwrap();
            snapshot(&conn)
        };
        let error = crate::Store::open(database.path())
            .err()
            .expect("incompatible original default must reject open");
        assert!(
            error
                .to_string()
                .contains("incompatible owned default sync_oplog.ts"),
            "{error}"
        );
        let conn = Connection::open(database.path()).unwrap();
        assert_eq!(snapshot(&conn), before);
        assert_eq!(scalar(&conn, "SELECT count(*) FROM schema_migrations"), 21);
        assert!(conn.prepare("SELECT id FROM entity_relations").is_err());
    }
}

#[test]
fn original_and_added_defaults_share_strict_public_admission() {
    for (table, old, new, field) in [
        (
            "schema_migrations",
            "applied_at DATETIME DEFAULT CURRENT_TIMESTAMP",
            "applied_at DATETIME",
            "applied_at",
        ),
        (
            "profiles",
            "role TEXT NOT NULL DEFAULT 'readwrite'",
            "role TEXT NOT NULL DEFAULT 'unexpected'",
            "role",
        ),
        (
            "entities",
            "aliases TEXT NOT NULL DEFAULT '[]'",
            "aliases TEXT NOT NULL DEFAULT '{}'",
            "aliases",
        ),
        (
            "import_state",
            "memory_count INTEGER NOT NULL DEFAULT 0",
            "memory_count INTEGER NOT NULL DEFAULT 99",
            "memory_count",
        ),
        (
            "query_log",
            "duration_ms INTEGER NOT NULL DEFAULT 0",
            "duration_ms INTEGER NOT NULL",
            "duration_ms",
        ),
        (
            "memories",
            "review_status TEXT NOT NULL DEFAULT 'approved'",
            "review_status TEXT NOT NULL DEFAULT 'staged'",
            "review_status",
        ),
        (
            "sessions",
            "summary TEXT NOT NULL",
            "summary TEXT NOT NULL DEFAULT NULL",
            "summary",
        ),
        (
            "rules",
            "content TEXT NOT NULL",
            "content TEXT NOT NULL DEFAULT (abs(-7))",
            "content",
        ),
        (
            "audit_log",
            "created_at DATETIME NOT NULL DEFAULT (datetime('now'))",
            "created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "created_at",
        ),
    ] {
        let database = OwnedDatabase::new();
        let before = {
            let conn = Connection::open(database.path()).unwrap();
            conn.execute_batch(&changed_table(SCHEMA, table, old, new))
                .unwrap();
            conn.execute_batch(INDEXES).unwrap();
            conn.execute("INSERT INTO sessions(id,summary,updated_at) VALUES ('kept','existing row','2000-01-01')", []).unwrap();
            snapshot(&conn)
        };
        let error = crate::Store::open(database.path())
            .err()
            .expect("incompatible default must reject");
        assert!(
            error
                .to_string()
                .contains(&format!("incompatible owned default {table}.{field}")),
            "{error}"
        );
        assert_eq!(
            snapshot(&Connection::open(database.path()).unwrap()),
            before
        );
    }
}

#[test]
fn every_declared_owned_default_rejects_a_changed_omission_value() {
    let facts = super::facts::owned().unwrap();
    let mut checked = 0;
    for (table, columns) in &facts.columns {
        for column in columns.iter().filter(|column| column.default.is_some()) {
            let database = OwnedDatabase::new();
            let before = {
                let conn = Connection::open(database.path()).unwrap();
                prefix(&conn, 36, true);
                // Rename preserves the original rows and their declaration.
                // Add a changed named field, then prove default admission runs
                // before later key/index/trigger mismatch diagnostics.
                conn.execute_batch(&format!(
                    "ALTER TABLE {table} RENAME COLUMN {} TO owned_original_{}; \
                     ALTER TABLE {table} ADD COLUMN {} {} DEFAULT 'owned-default-control';",
                    column.name, column.name, column.name, column.kind
                ))
                .unwrap();
                snapshot(&conn)
            };
            let error = crate::Store::open(database.path())
                .err()
                .expect("changed owned default must reject");
            assert!(
                error.to_string().contains(&format!(
                    "incompatible owned default {table}.{}",
                    column.name
                )),
                "{error}"
            );
            assert_eq!(
                snapshot(&Connection::open(database.path()).unwrap()),
                before
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 62, "every frozen declared omission contract");
}

#[test]
fn proven_old_query_default_preserves_rows_ddl_and_reopen_then_accepts_writes() {
    let database = OwnedDatabase::new();
    {
        let conn = Connection::open(database.path()).unwrap();
        let legacy = changed_table(
            SCHEMA,
            "query_log",
            "created_at DATETIME NOT NULL DEFAULT (datetime('now'))",
            "created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP",
        );
        conn.execute_batch(&legacy).unwrap();
        conn.execute_batch(INDEXES).unwrap();
        conn.execute_batch("INSERT INTO query_log(id,tool,created_at) VALUES ('kept','search','2000-01-01 00:00:00');").unwrap();
    }
    let first = {
        let store = crate::Store::open(database.path()).unwrap();
        store
            .set(
                "owned default remains writable",
                "global",
                "note",
                serde_json::Map::new(),
                false,
            )
            .unwrap();
        let conn = store.lock().unwrap();
        let default: String = conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info('query_log') WHERE name='created_at'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(default, "CURRENT_TIMESTAMP");
        let timestamp: String = conn
            .query_row("SELECT ts FROM sync_oplog LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert!(
            chrono::DateTime::parse_from_rfc3339(&timestamp).is_ok(),
            "{timestamp}"
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM query_log WHERE id='kept' AND created_at='2000-01-01 00:00:00'"
            ),
            1
        );
        snapshot(&conn)
    };
    for _ in 0..2 {
        let store = crate::Store::open(database.path()).unwrap();
        assert_eq!(snapshot(&store.lock().unwrap()), first);
    }
}
