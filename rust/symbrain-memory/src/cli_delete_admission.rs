//! Refuse delete rows whose Go hydration/error contract is not yet native.

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::BTreeMap;
use std::path::Path;

/// Inspects the existing row without migrations or mutations. Missing IDs are
/// supported; unknown schema, malformed JSON and noncanonical dates retain Go.
#[must_use]
pub fn direct_delete_supported(path: &Path, id: &str) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .is_ok_and(|conn| checked(&conn, id).is_ok())
}

pub(crate) fn checked(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.query_row("SELECT * FROM memories WHERE id=?", [id], |row| {
        for field in [
            "id",
            "content",
            "scope",
            "embedding_source",
            "embedding_model",
            "embedding_quantization",
            "created_by",
            "updated_by",
            "created_session",
            "updated_session",
            "consolidation_status",
            "tier",
            "review_status",
            "kind",
        ] {
            let _: String = row.get(field)?;
        }
        for field in ["consolidated_into_id", "superseded_by"] {
            let _: Option<String> = row.get(field)?;
        }
        let _: Option<Vec<u8>> = row.get("embedding_binary")?;
        let _: f64 = row.get("importance")?;
        let _: f64 = row.get("decay_factor")?;
        let _: i64 = row.get("access_count")?;
        for field in ["created_at", "updated_at"] {
            if !canonical_time(&row.get::<_, String>(field)?) {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        for field in [
            "valid_from",
            "valid_to",
            "expires_at",
            "last_access",
            "prev_access",
            "retired_at",
        ] {
            if row
                .get::<_, Option<String>>(field)?
                .is_some_and(|value| !canonical_time(&value))
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        let metadata: String = row.get("metadata")?;
        serde_json::from_str::<Option<BTreeMap<String, Option<String>>>>(&metadata)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let embedding: String = row.get("embedding")?;
        let values = serde_json::from_str::<Option<Vec<Option<f32>>>>(&embedding)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        if values
            .into_iter()
            .flatten()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(())
    })
    .optional()?;
    Ok(())
}

pub(crate) fn canonical_time(value: &str) -> bool {
    static PATTERN: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})? \+0000 UTC$").ok())
        .as_ref().is_some_and(|pattern| pattern.is_match(value)) && crate::gotime::parse(value).is_some()
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn malformed_hydration_refuses_delete_before_any_mutation() {
        for (field, value) in [
            ("metadata", "{\"a\":1}"),
            ("metadata", "{\"a\":1,\"a\":\"later\"}"),
            ("embedding", "invalid"),
            ("embedding", "[1e39]"),
            ("created_at", "invalid"),
        ] {
            let store = Store::open_in_memory().unwrap();
            let memory = store
                .set(
                    "hello world",
                    "global",
                    "reference",
                    serde_json::Map::new(),
                    false,
                )
                .unwrap();
            let conn = store.lock().unwrap();
            conn.execute(
                &format!("UPDATE memories SET {field}=? WHERE id=?"),
                rusqlite::params![value, memory.id],
            )
            .unwrap();
            let before: i64 = conn
                .query_row("SELECT COUNT(*) FROM sync_oplog", [], |row| row.get(0))
                .unwrap();
            drop(conn);
            assert!(store.delete_cli(&memory.id).is_err());
            let conn = store.lock().unwrap();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM sync_oplog", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                before
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}
