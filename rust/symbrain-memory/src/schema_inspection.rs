//! Read-only checks of actual columns against the schema shipped by this binary.

use rusqlite::Connection;

/// Reports missing columns, including columns of missing tables, in stable order.
///
/// Expected facts come from the same schema used to initialize the store. The
/// supplied database is only inspected; migration records cannot prove DDL.
///
/// # Errors
/// Returns a SQLite error if the expected schema or actual facts cannot be read.
pub fn missing_required_columns(actual: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(crate::schema::SCHEMA)?;
    let mut tables = expected.prepare(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let tables = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut missing = Vec::new();
    for table in tables {
        let required = columns(&expected, &table)?;
        let present = columns(actual, &table)?;
        for column in required {
            if !present.contains(&column) {
                missing.push(format!("{table}.{column}"));
            }
        }
    }
    missing.sort();
    Ok(missing)
}

fn columns(connection: &Connection, table: &str) -> Result<Vec<String>, rusqlite::Error> {
    // Table identifiers originate exclusively in the binary's own schema.
    let table = table.replace('"', "\"\"");
    connection
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_missing_ddl_despite_bookkeeping_without_repairing_database() {
        let conn = Connection::open_in_memory().unwrap();
        let mut legacy = crate::schema::SCHEMA.to_owned();
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
        for version in crate::schema::MIGRATIONS {
            conn.execute(
                "INSERT INTO schema_migrations(version) VALUES (?)",
                [version],
            )
            .unwrap();
        }
        let before = columns(&conn, "memories").unwrap();
        assert_eq!(
            conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        let expected = [
            "memories.consolidated_into_id",
            "memories.embedding_binary",
            "memories.embedding_dim",
            "memories.embedding_quantization",
            "memories.lsh_hash",
        ];
        assert_eq!(missing_required_columns(&conn).unwrap(), expected);
        assert_eq!(columns(&conn, "memories").unwrap(), before);
        assert_eq!(missing_required_columns(&conn).unwrap(), expected);
    }

    #[test]
    fn healthy_store_and_missing_table_are_distinguished() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::schema::SCHEMA).unwrap();
        assert!(missing_required_columns(&conn).unwrap().is_empty());
        conn.execute_batch("DROP TABLE schema_migrations").unwrap();
        assert_eq!(
            missing_required_columns(&conn).unwrap(),
            ["schema_migrations.applied_at", "schema_migrations.version"]
        );
    }
}
