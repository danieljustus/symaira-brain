//! #649: a Go-created store whose bookkeeping claims every migration but whose
//! `memories` table predates the later columns must open, repair and stay usable.

use super::test_support::*;
use crate::schema::MIGRATIONS;
use rusqlite::Connection;

/// Tables as an old Go release created them: permissive `''`/`'{}'` omission
/// defaults on the original columns and none of the later `ALTER` columns.
const LEGACY_TABLES: &str = "\
CREATE TABLE memories (id TEXT PRIMARY KEY, content TEXT NOT NULL, \
scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', \
embedding TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL, \
updated_at DATETIME NOT NULL, kind TEXT NOT NULL DEFAULT '');\
CREATE TABLE rules (id TEXT PRIMARY KEY, content TEXT NOT NULL, \
scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', \
created_at DATETIME);\
CREATE TABLE schema_migrations (version TEXT PRIMARY KEY, \
applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);\
INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) \
VALUES ('kept','pre-existing note','global','{}','[]','2000-01-02 03:04:05','2000-01-02 03:04:05');";

const FIVE_COLUMNS: [&str; 5] = [
    "embedding_binary",
    "embedding_dim",
    "embedding_quantization",
    "lsh_hash",
    "consolidated_into_id",
];

fn legacy_database() -> OwnedDatabase {
    let database = OwnedDatabase::new();
    let conn = Connection::open(database.path()).unwrap();
    conn.execute_batch(LEGACY_TABLES).unwrap();
    for version in MIGRATIONS {
        conn.execute(
            "INSERT INTO schema_migrations(version) VALUES (?)",
            [version],
        )
        .unwrap();
    }
    for column in FIVE_COLUMNS {
        assert!(
            conn.prepare(&format!("SELECT {column} FROM memories"))
                .is_err()
        );
    }
    database
}

#[test]
fn legacy_go_store_with_full_ledger_opens_and_gains_missing_columns() {
    let database = legacy_database();
    for _ in 0..3 {
        let store = crate::Store::open(database.path()).unwrap();
        let conn = store.lock().unwrap();
        for column in FIVE_COLUMNS {
            conn.prepare(&format!("SELECT {column} FROM memories"))
                .unwrap();
        }
        assert_eq!(
            scalar(&conn, "SELECT count(*) FROM schema_migrations"),
            i64::try_from(MIGRATIONS.len()).unwrap()
        );
        assert_eq!(
            conn.query_row("SELECT content FROM memories WHERE id='kept'", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            "pre-existing note"
        );
    }
}

#[test]
fn repaired_legacy_go_store_serves_set_and_search() {
    let database = legacy_database();
    let store = crate::Store::open(database.path()).unwrap();
    let memory = store
        .set(
            "written after repair",
            "global",
            "user",
            serde_json::Map::new(),
            false,
        )
        .unwrap();
    assert_eq!(
        store.get(&memory.id).unwrap().unwrap().content,
        "written after repair"
    );
    let embedding = crate::embedding::local_hash_vector(&memory.content);
    assert!(
        !store
            .search_ranked(&embedding, "hash-fallback", "global", 5)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn legacy_default_allowance_stays_narrow() {
    // A different default on an original column is still an incompatible contract.
    let database = OwnedDatabase::new();
    {
        let conn = Connection::open(database.path()).unwrap();
        conn.execute_batch(&LEGACY_TABLES.replace(
            "scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', \
embedding TEXT NOT NULL DEFAULT ''",
            "scope TEXT NOT NULL DEFAULT 'surprise', metadata TEXT NOT NULL DEFAULT '{}', \
embedding TEXT NOT NULL DEFAULT ''",
        ))
        .unwrap();
    }
    let error = crate::Store::open(database.path())
        .err()
        .expect("unexpected default must reject");
    assert!(
        error
            .to_string()
            .contains("incompatible owned default memories.scope"),
        "{error}"
    );
}
