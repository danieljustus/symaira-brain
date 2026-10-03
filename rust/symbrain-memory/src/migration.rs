//! Atomic schema initialization and idempotent legacy-column repair.
//!
//! Migration bookkeeping alone is insufficient: #649 reported applied entries
//! while five required columns were absent. Inspect the actual table each open.

use crate::schema::{COLUMN_PARITY, INDEXES, MIGRATIONS, SCHEMA};
use crate::{Store, StoreError};
use rusqlite::Connection;
use std::time::Duration;

pub(crate) fn configure(mut conn: Connection) -> Result<Store, StoreError> {
    conn.busy_timeout(Duration::from_secs(5))?;
    // journal_mode cannot be changed inside a transaction.
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON;",
    )?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    apply_column_parity(&tx)?;
    // Build indexes after legacy columns exist, and commit them together with
    // schema and bookkeeping. Failed repair must not publish applied entries.
    tx.execute_batch(INDEXES)?;
    for version in MIGRATIONS {
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?)",
            [version],
        )?;
    }
    tx.commit()?;
    Ok(Store {
        conn: std::sync::Mutex::new(conn),
    })
}

fn apply_column_parity(conn: &Connection) -> Result<(), StoreError> {
    for (table, column, definition) in COLUMN_PARITY {
        let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let present = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == column);
        drop(statement);
        if !present {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_the_five_649_columns_despite_all_migrations_claimed() {
        let conn = Connection::open_in_memory().unwrap();
        let mut legacy = SCHEMA.to_owned();
        for declaration in [
            " embedding_binary BLOB,\n",
            " embedding_dim INTEGER NOT NULL DEFAULT 0,\n",
            " embedding_quantization TEXT NOT NULL DEFAULT '',\n",
            " lsh_hash INTEGER NOT NULL DEFAULT 0,\n",
            " consolidated_into_id TEXT REFERENCES memories(id) ON DELETE SET NULL,\n",
        ] {
            assert!(legacy.contains(declaration));
            legacy = legacy.replace(declaration, "");
        }
        conn.execute_batch(&legacy).unwrap();
        for version in MIGRATIONS {
            conn.execute(
                "INSERT INTO schema_migrations(version) VALUES (?)",
                [version],
            )
            .unwrap();
        }
        let store = configure(conn).unwrap();
        for _ in 0..2 {
            let conn = store.lock().unwrap();
            apply_column_parity(&conn).unwrap();
            for column in [
                "embedding_binary",
                "embedding_dim",
                "embedding_quantization",
                "lsh_hash",
                "consolidated_into_id",
            ] {
                conn.prepare(&format!("SELECT {column} FROM memories"))
                    .unwrap();
            }
        }
        let memory = store
            .set(
                "schema repair remains writable",
                "global",
                "user",
                serde_json::Map::new(),
                false,
            )
            .unwrap();
        assert_eq!(
            store.get(&memory.id).unwrap().unwrap().content,
            memory.content
        );
        let embedding = crate::embedding::local_hash_vector(&memory.content);
        assert_eq!(
            store
                .search_ranked(&embedding, "hash-fallback", "global", 5)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn later_ddl_failure_rolls_back_columns_and_migration_entries() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE memories(id TEXT PRIMARY KEY); CREATE TABLE rules(id TEXT PRIMARY KEY); CREATE TABLE schema_migrations(version TEXT PRIMARY KEY);").unwrap();
        {
            let tx = conn.transaction().unwrap();
            apply_column_parity(&tx).unwrap();
            tx.execute(
                "INSERT INTO schema_migrations(version) VALUES ('034_activity_store')",
                [],
            )
            .unwrap();
            assert!(
                tx.execute_batch("CREATE INDEX intentional_missing_column ON memories(absent);")
                    .is_err()
            );
            // Drop rolls back the same transaction configure uses.
        }
        assert_eq!(
            conn.query_row("SELECT count(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(
            conn.prepare("SELECT embedding_binary FROM memories")
                .is_err()
        );
    }
}
