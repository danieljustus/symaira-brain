//! Store-level contract checks against the shipped database representations.
//!
//! The shipped implementation stores timestamps as `2006-01-02 15:04:05.999999
//! +0000 UTC`, identifiers as UUIDs, `embedding` as a JSON array and a
//! SHA-256 `content_hash`. A native row has to look the same, and a row the
//! shipped implementation wrote has to be readable here.

use std::path::Path;

use symbrain_memory::{SetOptions, Store};

fn options(actor: &str) -> SetOptions {
    SetOptions {
        actor: actor.to_owned(),
        ..SetOptions::default()
    }
}

/// Reads one column from the database file, as text.
fn column(path: &Path, sql: &str) -> String {
    let connection = rusqlite::Connection::open(path).expect("open for assertions");
    connection
        .query_row(sql, [], |row| row.get::<_, String>(0))
        .expect("column")
}

fn store_in(directory: &std::path::Path) -> (Store, std::path::PathBuf) {
    let path = directory.join("memory.db");
    (Store::open(&path).expect("store"), path)
}

/// Writes one contract row into a fresh database and returns the store, the
/// database path and the temporary directory that owns it.
fn contract_store() -> (Store, std::path::PathBuf, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "symbrain-store-contract-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("tempdir");
    let (store, path) = store_in(&directory);
    store
        .set_with_options(
            "contract row",
            "global",
            "user",
            serde_json::Map::new(),
            &options("cli:symbrain"),
        )
        .expect("set");
    (store, path, directory)
}

#[test]
fn written_row_uses_shipped_identifiers() {
    let (store, _path, directory) = contract_store();
    let rows = store.list("", 10).expect("list");
    let id = rows[0].id.clone();
    let parts = id.split('-').collect::<Vec<_>>();
    assert_eq!(parts.len(), 5, "uuid shape: {id}");
    assert_eq!(
        parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12]
    );
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        "{id}"
    );
    assert_eq!(parts[2].chars().next(), Some('4'), "uuid version: {id}");
    assert!(
        matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
        "uuid variant: {id}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn written_row_uses_shipped_timestamps() {
    let (_store, path, directory) = contract_store();
    let created = column(&path, "SELECT created_at FROM memories LIMIT 1");
    assert!(created.ends_with(" +0000 UTC"), "{created}");
    assert_eq!(&created[10..11], " ");
    if let Some((_, fraction)) = created.split(' ').nth(1).and_then(|t| t.split_once('.')) {
        assert!(fraction.len() <= 6, "microseconds at most: {created}");
        assert!(
            !fraction.ends_with('0'),
            "trailing zeros trimmed: {created}"
        );
    }
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn written_row_uses_shipped_columns() {
    let (_store, path, directory) = contract_store();
    assert_eq!(
        column(&path, "SELECT content_hash FROM memories LIMIT 1"),
        "f0a362523aea59bd15e13e7f36e59b0a2b91fe262e381a47e2f503f2ea8155b0"
    );
    for (sql, expected) in [
        (
            "SELECT CAST(embedding_dim AS TEXT) FROM memories LIMIT 1",
            "768",
        ),
        (
            "SELECT embedding_source FROM memories LIMIT 1",
            "hash-fallback",
        ),
        (
            "SELECT CAST(importance AS TEXT) FROM memories LIMIT 1",
            "0.0",
        ),
        (
            "SELECT CAST(access_count AS TEXT) FROM memories LIMIT 1",
            "1",
        ),
        (
            "SELECT CAST(decay_factor AS TEXT) FROM memories LIMIT 1",
            "1.0",
        ),
        ("SELECT created_by FROM memories LIMIT 1", "cli:symbrain"),
        ("SELECT updated_by FROM memories LIMIT 1", "cli:symbrain"),
        ("SELECT consolidation_status FROM memories LIMIT 1", "raw"),
        ("SELECT review_status FROM memories LIMIT 1", "approved"),
        ("SELECT tier FROM memories LIMIT 1", "long_term"),
    ] {
        assert_eq!(column(&path, sql), expected, "{sql}");
    }
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn written_row_uses_shipped_embedding() {
    let (store, path, directory) = contract_store();
    // The embedding column holds the shipped hash-fallback vector as JSON:
    // 768 normalized values, so both stores share one embedding space.
    let embedding = column(&path, "SELECT embedding FROM memories LIMIT 1");
    let vector: Vec<f32> = serde_json::from_str(&embedding).expect("embedding JSON");
    assert_eq!(vector.len(), 768);
    let norm: f64 = vector
        .iter()
        .map(|value| f64::from(*value * *value))
        .sum::<f64>()
        .sqrt();
    assert!((norm - 1.0).abs() < 1e-6, "norm {norm}");
    assert!(
        vector.iter().filter(|value| **value != 0.0).count() > 1,
        "hash fallback spreads the content over several dimensions"
    );

    // The row is readable again through the native path.
    let rows = store.list("", 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, "contract row");
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn shipped_timestamp_rendering_is_readable() {
    let directory =
        std::env::temp_dir().join(format!("symbrain-store-shipped-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("tempdir");
    let (store, path) = store_in(&directory);
    {
        let connection = rusqlite::Connection::open(&path).expect("open");
        connection
            .execute_batch(
                "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at,kind) \
                 VALUES('4c7d91e7-f6fb-49a1-8cc4-c64a79d02feb','shipped row','global','{}','',\
                 '2026-09-18 13:19:17.139363 +0000 UTC','2026-09-18 13:19:17.139363 +0000 UTC','user');",
            )
            .expect("insert");
    }
    let rows = store.list("", 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, "shipped row");
    assert_eq!(
        rows[0].created_at.to_rfc3339(),
        "2026-09-18T13:19:17.139363+00:00"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn missing_shipped_columns_are_added_to_an_existing_database() {
    let directory =
        std::env::temp_dir().join(format!("symbrain-store-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("tempdir");
    let path = directory.join("memory.db");
    {
        let connection = rusqlite::Connection::open(&path).expect("open");
        connection
            .execute_batch(
                "CREATE TABLE memories (id TEXT PRIMARY KEY, content TEXT NOT NULL, \
                 scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', \
                 embedding TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL, \
                 updated_at DATETIME NOT NULL, kind TEXT NOT NULL DEFAULT '');\
                 CREATE TABLE rules (id TEXT PRIMARY KEY, content TEXT NOT NULL, \
                 scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', \
                 created_at DATETIME);",
            )
            .expect("legacy schema");
    }
    let _store = Store::open(&path).expect("store opens a legacy database");
    for name in [
        "embedding_dim",
        "lsh_hash",
        "consolidated_into_id",
        "embedding_binary",
        "embedding_quantization",
    ] {
        assert_eq!(
            column(
                &path,
                &format!(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM pragma_table_info('memories') WHERE name='{name}'"
                )
            ),
            "1",
            "memories.{name}"
        );
    }
    for name in ["updated_at", "created_by", "updated_by"] {
        assert_eq!(
            column(
                &path,
                &format!(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM pragma_table_info('rules') WHERE name='{name}'"
                )
            ),
            "1",
            "rules.{name}"
        );
    }
    std::fs::remove_dir_all(&directory).ok();
}

/// `memories_fts` is only useful with its `memories_ai`/`_au` triggers: the
/// table can exist and never receive a row. This exercises the trigger chain
/// end to end, through the shipped query form.
#[test]
fn a_native_row_reaches_the_shipped_fts_index() {
    let (store, path, directory) = contract_store();
    let _ = store;
    let hits = column(
        &path,
        "SELECT CAST(COUNT(*) AS TEXT) FROM memories_fts WHERE memories_fts MATCH 'contract'",
    );
    assert_eq!(hits, "1", "the FTS index never received the native row");
    std::fs::remove_dir_all(&directory).ok();
}
