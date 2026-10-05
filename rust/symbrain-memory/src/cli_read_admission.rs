//! Keep unsupported stored read JSON on Go without mutating historical stores.

use rusqlite::{Connection, OpenFlags};
use std::{collections::BTreeMap, path::Path};

/// Returns whether stored JSON is in the native CLI decoder's admitted domain.
/// Missing tables/columns are left to the existing transactional migration.
#[must_use]
pub fn direct_reads_supported(path: &Path, verb: &str) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .is_ok_and(|conn| checked(&conn, verb).unwrap_or(false))
}

fn checked(conn: &Connection, verb: &str) -> rusqlite::Result<bool> {
    let table = if verb == "rules" { "rules" } else { "memories" };
    let mut columns = conn.prepare("SELECT name FROM pragma_table_info(?)")?;
    let names = columns
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // ponytail: conservatively inspect the entire table. Narrow to the exact
    // selected rows only when their full Go hydration/diagnostics are ported.
    for field in ["metadata", "embedding"] {
        if (field == "embedding" && verb != "search") || !names.iter().any(|name| name == field) {
            continue;
        }
        let mut statement = conn.prepare(&format!("SELECT {field} FROM {table}"))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let text: String = row.get(0)?;
            let supported = if field == "metadata" {
                // Root/member null and Go's error diagnostics retain Go too.
                serde_json::from_str::<BTreeMap<String, String>>(&text).is_ok()
            } else {
                serde_json::from_str::<Vec<f32>>(&text)
                    .is_ok_and(|values| values.iter().all(|value| value.is_finite()))
            };
            if !supported {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_admission_rejects_unsupported_json_without_changing_rows_or_schema() {
        let conn = Connection::open_in_memory().unwrap();
        // A pre-table historical store remains eligible for native migration.
        assert!(checked(&conn, "list").unwrap());
        conn.execute_batch(
            "CREATE TABLE memories(metadata TEXT, embedding TEXT); \
                            CREATE TABLE rules(metadata TEXT); \
                            INSERT INTO memories VALUES('{}','[]'); \
                            INSERT INTO rules VALUES('{}');",
        )
        .unwrap();
        for verb in ["list", "rules", "search"] {
            assert!(checked(&conn, verb).unwrap());
        }
        for value in [
            "broken",
            "",
            "[]",
            "false",
            "{\"n\":1}",
            "{\"n\":1,\"n\":\"later\"}",
            "null",
            "{\"n\":null}",
        ] {
            for table in ["memories", "rules"] {
                conn.execute(&format!("UPDATE {table} SET metadata=?"), [value])
                    .unwrap();
                let verb = if table == "rules" { "rules" } else { "list" };
                assert!(!checked(&conn, verb).unwrap(), "{verb}: {value}");
                let after: String = conn
                    .query_row(&format!("SELECT metadata FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(after, value);
                conn.execute(&format!("UPDATE {table} SET metadata='{{}}'"), [])
                    .unwrap();
            }
        }
        for value in ["broken", "", "{}", "false", "[\"x\"]", "[1e39]", "[null]"] {
            conn.execute("UPDATE memories SET embedding=?", [value])
                .unwrap();
            assert!(!checked(&conn, "search").unwrap(), "{value}");
            assert!(
                checked(&conn, "list").unwrap(),
                "lite reads do not hydrate embedding"
            );
        }
        conn.execute("UPDATE memories SET metadata='broken'", [])
            .unwrap();
        assert!(
            conn.query_row(
                "SELECT metadata FROM memories",
                [],
                |row| crate::rows::json::<serde_json::Map<String, serde_json::Value>>(row, 0)
            )
            .is_err()
        );
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 2);
    }
}
