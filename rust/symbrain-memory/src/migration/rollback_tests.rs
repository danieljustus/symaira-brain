//! Public opens must roll back row repairs, FTS DDL, indexes and marker writes.

use super::test_support::*;
use rusqlite::Connection;

#[test]
fn actual_rule_and_relation_abort_callbacks_preserve_the_whole_database() {
    for (version, trigger) in [
        (
            "006_provenance",
            "CREATE TRIGGER reject_rule BEFORE UPDATE ON rules BEGIN SELECT RAISE(ABORT,'owned rule control'); END;",
        ),
        (
            "022_entity_relation_provenance",
            "CREATE TRIGGER reject_relation BEFORE UPDATE ON entity_relations BEGIN SELECT RAISE(ABORT,'owned relation control'); END;",
        ),
    ] {
        let database = OwnedDatabase::new();
        let before = {
            let conn = Connection::open(database.path()).unwrap();
            before_migration(&conn, version, true);
            conn.execute_batch(trigger).unwrap();
            snapshot(&conn)
        };
        let error = crate::Store::open(database.path())
            .err()
            .expect("actual callback must abort open");
        assert!(error.to_string().contains("owned "), "{error}");
        assert_eq!(
            snapshot(&Connection::open(database.path()).unwrap()),
            before
        );
    }
}

#[test]
fn ignored_backfills_and_marker_insert_cannot_publish_false_completion() {
    for (version, trigger, diagnostic) in [
        (
            "006_provenance",
            "CREATE TRIGGER ignore_rule BEFORE UPDATE ON rules BEGIN SELECT RAISE(IGNORE); END;",
            "rule timestamp",
        ),
        (
            "022_entity_relation_provenance",
            "CREATE TRIGGER ignore_relation BEFORE UPDATE ON entity_relations BEGIN SELECT RAISE(IGNORE); END;",
            "relation identity",
        ),
        (
            "022_entity_relation_provenance",
            "CREATE TRIGGER ignore_relation_time BEFORE UPDATE OF updated_at ON entity_relations BEGIN SELECT RAISE(IGNORE); END;",
            "relation timestamp",
        ),
        (
            "006_provenance",
            "CREATE TRIGGER ignore_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(IGNORE); END;",
            "migration marker",
        ),
    ] {
        let database = OwnedDatabase::new();
        let before = {
            let conn = Connection::open(database.path()).unwrap();
            before_migration(&conn, version, true);
            conn.execute_batch(trigger).unwrap();
            snapshot(&conn)
        };
        let error = crate::Store::open(database.path())
            .err()
            .expect("false completion must stop");
        assert!(error.to_string().contains(diagnostic), "{error}");
        assert_eq!(
            snapshot(&Connection::open(database.path()).unwrap()),
            before
        );
    }
}

#[test]
fn missing_check_constraint_is_not_certified_by_column_presence() {
    let database = OwnedDatabase::new();
    let before = {
        let conn = Connection::open(database.path()).unwrap();
        prefix(&conn, 36, true);
        let definition: String = conn
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='activity_episodes'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let weakened = definition.replace("CHECK (confidence >= 0 AND confidence <= 1)", "");
        assert_ne!(weakened, definition);
        conn.execute_batch("DROP TABLE activity_episodes;").unwrap();
        conn.execute_batch(&weakened).unwrap();
        snapshot(&conn)
    };
    let error = crate::Store::open(database.path())
        .err()
        .expect("missing constraint must stop");
    assert!(error.to_string().contains("activity_episodes"), "{error}");
    assert_eq!(
        snapshot(&Connection::open(database.path()).unwrap()),
        before
    );
}

#[test]
fn later_wrong_index_rolls_back_uuid_backfill_fts_upgrade_and_new_markers() {
    let database = OwnedDatabase::new();
    let (before, marker_count) = {
        let conn = Connection::open(database.path()).unwrap();
        let marker_count = before_migration(&conn, "022_entity_relation_provenance", true);
        assert!(conn.prepare("SELECT id FROM entity_relations").is_err());
        conn.execute("CREATE INDEX idx_query_log_actor ON rules(content)", [])
            .unwrap();
        (snapshot(&conn), marker_count)
    };
    let error = crate::Store::open(database.path())
        .err()
        .expect("wrong index must stop repair");
    assert!(error.to_string().contains("idx_query_log_actor"), "{error}");
    let conn = Connection::open(database.path()).unwrap();
    assert_eq!(snapshot(&conn), before);
    assert_eq!(
        scalar(&conn, "SELECT count(*) FROM schema_migrations"),
        marker_count
    );
    assert!(conn.prepare("SELECT id FROM entity_relations").is_err());
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
        ),
        0
    );
}

#[test]
fn duplicate_nonempty_relation_ids_are_preserved_and_unique_repair_aborts() {
    let database = OwnedDatabase::new();
    let before = {
        let conn = Connection::open(database.path()).unwrap();
        prefix(&conn, 36, true);
        conn.execute_batch("DROP INDEX idx_entity_relations_relation_id; UPDATE entity_relations SET id='caller-collision';").unwrap();
        snapshot(&conn)
    };
    assert!(crate::Store::open(database.path()).is_err());
    let conn = Connection::open(database.path()).unwrap();
    assert_eq!(snapshot(&conn), before);
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM entity_relations WHERE id='caller-collision'"
        ),
        2
    );
}

#[test]
fn unknown_sync_trigger_is_preserved_instead_of_replaced() {
    let database = OwnedDatabase::new();
    let before = {
        let conn = Connection::open(database.path()).unwrap();
        prefix(&conn, 20, true);
        conn.execute_batch("CREATE TRIGGER trg_memories_oplog_insert AFTER INSERT ON memories BEGIN SELECT 'custom sync owner'; END;").unwrap();
        snapshot(&conn)
    };
    assert!(crate::Store::open(database.path()).is_err());
    assert_eq!(
        snapshot(&Connection::open(database.path()).unwrap()),
        before
    );
}

#[cfg(unix)]
#[test]
fn successful_public_open_secures_existing_database_without_rewriting_its_rows() {
    use std::os::unix::fs::PermissionsExt;
    let database = OwnedDatabase::new();
    {
        let conn = Connection::open(database.path()).unwrap();
        prefix(&conn, 36, true);
    }
    std::fs::set_permissions(database.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let store = crate::Store::open(database.path()).unwrap();
    assert_eq!(
        std::fs::metadata(database.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        store.get("memory").unwrap().unwrap().content,
        "running quickly"
    );
}
