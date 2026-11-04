//! Frozen CLI direct-write persistence, separate from the MCP migration surface.

use crate::{Store, StoreError};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

impl Store {
    /// Deletes through the CLI service's access-feedback and audit sequence.
    ///
    /// # Errors
    /// Returns a SQLite error; audit errors do not fail a completed deletion.
    pub fn delete_cli(&self, id: &[u8]) -> Result<bool, StoreError> {
        self.delete_service_bytes(id, true)
    }

    pub(crate) fn delete_service(&self, id: &str, audit_enabled: bool) -> Result<bool, StoreError> {
        self.delete_service_bytes(id.as_bytes(), audit_enabled)
    }

    /// The identifier is bound as the exact `TEXT` bytes Go binds, valid
    /// UTF-8 or not.
    fn delete_service_bytes(
        &self,
        id_bytes: &[u8],
        audit_enabled: bool,
    ) -> Result<bool, StoreError> {
        let id = RawText(id_bytes);
        let conn = self.lock()?;
        // `GetMemory`: the full-row hydration, with its scan diagnostics.
        let loaded = conn
            .query_row(
                &format!(
                    "SELECT {} FROM memories WHERE id = ?",
                    crate::search_rows::SEARCH_COLUMNS
                ),
                [&id],
                crate::SearchRow::from_row,
            )
            .optional()?;
        if loaded.is_none() {
            return Ok(false);
        }
        conn.execute("UPDATE memories SET access_count=access_count+1,prev_access=last_access,last_access=? WHERE id=?",
            params![timestamp(), id])?;
        // Go reads the audit identity in DeleteMemory after GetMemory's access
        // update. If another process already deleted it, DeleteMemory succeeds.
        let identity: Option<(String, String, String)> = conn.query_row(
            "SELECT COALESCE(scope,''),COALESCE(created_by,''),COALESCE(created_session,'') FROM memories WHERE id=?", [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((scope, author, session)) = identity else {
            return Ok(true);
        };
        conn.execute("DELETE FROM memories WHERE id=?", [&id])?;
        if audit_enabled {
            audit(
                &conn,
                "delete",
                &String::from_utf8_lossy(id_bytes),
                &scope,
                &session,
                &author,
                "",
            );
        }
        Ok(true)
    }
}

/// A `TEXT` parameter from raw bytes (SQLite does not require UTF-8).
struct RawText<'a>(&'a [u8]);

impl rusqlite::ToSql for RawText<'_> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(
            rusqlite::types::ValueRef::Text(self.0),
        ))
    }
}

pub(crate) fn audit(
    conn: &Connection,
    action: &str,
    id: &str,
    scope: &str,
    session: &str,
    author: &str,
    detail: &str,
) {
    let nonempty = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    let _ = conn.execute("INSERT INTO audit_log(id,action,memory_id,scope,session,actor,detail,created_at) VALUES(?,?,?,?,?,?,?,?)",
        params![crate::write::new_uuid(), action, nonempty(id), nonempty(scope), nonempty(session), nonempty(author), nonempty(detail), timestamp()]);
}

pub(crate) fn go_json(value: &impl serde::Serialize) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)
        .map_err(|error| StoreError::Invalid(error.to_string()))?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

// encoding/json uses the float32 shortest representation with its own fixed/
// scientific boundary. Keep negative zero and normalize exponent signs only.
pub(crate) fn f32_json(value: f32) -> String {
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-6..1e21).contains(&magnitude) {
        let scientific = format!("{value:e}");
        let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        format!("{mantissa}e{exponent:+}")
    } else {
        format!("{value}")
    }
}

pub(crate) fn embedding_json(vector: &[f32]) -> String {
    let values = vector
        .iter()
        .map(|value| f32_json(*value))
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

pub(crate) fn sign_bits(vector: &[f32]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 96];
    for (index, value) in vector.iter().take(768).enumerate() {
        if *value <= 0.0 {
            bytes[index / 8] |= 1 << (index % 8);
        }
    }
    bytes
}

pub(crate) fn timestamp() -> String {
    let now = Utc::now();
    let base = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = now.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base} +0000 UTC")
    } else {
        let fraction = format!("{nanos:09}");
        format!("{base}.{} +0000 UTC", fraction.trim_end_matches('0'))
    }
}
