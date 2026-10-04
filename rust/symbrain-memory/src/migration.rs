//! Atomic schema initialization and idempotent legacy-column repair.
//!
//! Migration bookkeeping alone is insufficient: #649 reported applied entries
//! while five required columns were absent. Inspect the actual table each open.

use crate::schema::{COLUMN_PARITY, INDEXES, MIGRATIONS, SCHEMA};
use crate::{Store, StoreError};
use rusqlite::{Connection, TransactionBehavior};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ConfigurePhase {
    Connection,
    SecureDelete,
    Migration,
}

pub(crate) struct ConfigureError {
    pub phase: ConfigurePhase,
    pub error: StoreError,
}

pub(crate) fn configure(conn: Connection) -> Result<Store, StoreError> {
    configure_with_phase(conn).map_err(|error| error.error)
}

pub(crate) fn configure_with_phase(conn: Connection) -> Result<Store, ConfigureError> {
    let error = |phase, error| ConfigureError { phase, error };
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| error(ConfigurePhase::Connection, e.into()))?;
    // Go's DSN configures these before the separately labelled secure-delete
    // step. Keep the same connection and original atomic migration below.
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
        .map_err(|e| error(ConfigurePhase::Connection, e.into()))?;
    conn.execute_batch("PRAGMA secure_delete=ON;")
        .map_err(|e| error(ConfigurePhase::SecureDelete, e.into()))?;
    migrate(conn).map_err(|e| error(ConfigurePhase::Migration, e))
}

fn migrate(mut conn: Connection) -> Result<Store, StoreError> {
    // Reserve the writer before inspecting schema. IMMEDIATE retains the
    // existing bounded BUSY wait and prevents a read-snapshot upgrade.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(SCHEMA)?;
    apply_column_parity(&tx)?;
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
    fn concurrent_public_opens_and_writes_preserve_data_without_snapshot_upgrade() {
        use std::sync::{Arc, Barrier};
        let mut entropy = [0_u8; 8];
        getrandom::fill(&mut entropy).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "symbrain-schema-concurrency-{:016x}",
            u64::from_le_bytes(entropy),
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("memory.db");
        let id = {
            let store = Store::open(&path).unwrap();
            store
                .set(
                    "existing Unicode 世界 < & > remains",
                    "global",
                    "user",
                    serde_json::Map::new(),
                    false,
                )
                .unwrap()
                .id
        };
        // Real public openers use independent SQLite connections. Repeated
        // barriers force the previously failing concurrent read/write upgrade.
        for round in 0..10 {
            let barrier = Arc::new(Barrier::new(8));
            let jobs = (0..8)
                .map(|worker| {
                    let path = path.clone();
                    let id = id.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        let store = Store::open(&path).unwrap();
                        assert_eq!(
                            store.get(&id).unwrap().unwrap().content,
                            "existing Unicode 世界 < & > remains"
                        );
                        store
                            .set(
                                &format!("round {round} worker {worker}"),
                                "global",
                                "user",
                                serde_json::Map::new(),
                                false,
                            )
                            .unwrap();
                    })
                })
                .collect::<Vec<_>>();
            for job in jobs {
                job.join().unwrap();
            }
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.list("global", 1000).unwrap().len(), 81);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn later_ddl_failure_rolls_back_columns_and_migration_entries() {
        let mut entropy = [0_u8; 8];
        getrandom::fill(&mut entropy).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "symbrain-schema-rollback-{:016x}",
            u64::from_le_bytes(entropy),
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("memory.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE memories(id TEXT PRIMARY KEY); CREATE TABLE rules(id TEXT PRIMARY KEY); CREATE TABLE schema_migrations(version TEXT PRIMARY KEY);").unwrap();
        }
        // Invoke the real public opener: schema and parity changes precede the
        // index failure because this deliberately corrupt legacy table lacks
        // original scope/content columns that additive repair cannot invent.
        assert!(Store::open(&path).is_err());
        {
            let conn = Connection::open(&path).unwrap();
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
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='activity_segments'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
