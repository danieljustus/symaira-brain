//! Disposable historical fixtures from the owned, immutable SQL resources.

use super::catalog;
use rusqlite::{Connection, types::Value};
use std::path::{Path, PathBuf};

pub(super) const CREATED: &str = "2000-01-02 03:04:05";

pub(super) fn prefix(conn: &Connection, end: usize, seed: bool) {
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);").unwrap();
    for (index, (version, resource)) in catalog::STEPS.iter().enumerate().take(end + 1) {
        conn.execute_batch(resource).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations(version) VALUES (?)",
            [version],
        )
        .unwrap();
        if seed {
            match index {
                0 => conn.execute_batch("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('memory','running quickly','global','{}','[]','2000-01-02 03:04:05','2000-01-02 03:04:05'); INSERT INTO rules(id,content,scope,metadata,created_at) VALUES ('rule','keep unchanged','global','{}','2000-01-02 03:04:05');").unwrap(),
                7 => entities(conn),
                19 => conn.execute_batch("INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_at) VALUES ('a','b','knows','2000-01-02 03:04:05'),('b','a','knows','2000-01-02 03:04:05');").unwrap(),
                _ => {}
            }
        }
    }
}

/// Materialize the real prefix strictly before the named pending effect.
pub(super) fn before_migration(conn: &Connection, version: &str, seed: bool) -> i64 {
    let count = catalog::STEPS
        .iter()
        .position(|(name, _)| *name == version)
        .expect("fixture cutoff must name an owned migration");
    prefix(
        conn,
        count
            .checked_sub(1)
            .expect("fixture needs an existing base"),
        seed,
    );
    let count = i64::try_from(count).unwrap();
    assert_eq!(
        scalar(conn, "SELECT count(*) FROM schema_migrations"),
        count
    );
    let already_applied: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=?)",
            [version],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !already_applied,
        "named fixture effect must still be pending"
    );
    count
}

pub(super) fn entities(conn: &Connection) {
    conn.execute_batch("INSERT INTO entities(id,name,created_at,updated_at) VALUES ('a','Alice','2000-01-02 03:04:05','2000-01-02 03:04:05'),('b','Bob','2000-01-02 03:04:05','2000-01-02 03:04:05');").unwrap();
}

pub(super) fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// Capture all schema definitions and all table cells, including FTS shadows.
/// This compares real committed state rather than a selected column subset.
pub(super) fn snapshot(conn: &Connection) -> Vec<(String, Vec<Vec<Value>>)> {
    let definitions = conn
        .prepare("SELECT name,type,COALESCE(sql,'') FROM sqlite_schema ORDER BY name")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    definitions
        .into_iter()
        .map(|(name, kind, sql)| {
            let rows = if kind == "table" {
                let quoted = format!("\"{}\"", name.replace('"', "\"\""));
                let mut statement = conn.prepare(&format!("SELECT * FROM {quoted}")).unwrap();
                let count = statement.column_count();
                let mut rows = statement
                    .query_map([], |row| {
                        (0..count)
                            .map(|index| row.get::<_, Value>(index))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                rows.sort_by_cached_key(|row| format!("{row:?}"));
                rows
            } else {
                Vec::new()
            };
            (format!("{name}\n{kind}\n{sql}"), rows)
        })
        .collect()
}

pub(super) struct OwnedDatabase(PathBuf);

impl OwnedDatabase {
    pub fn new() -> Self {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "symbrain-historical-{:032x}",
            u128::from_le_bytes(entropy)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory.join("memory.db"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for OwnedDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}
