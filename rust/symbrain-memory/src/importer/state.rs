use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::{Store, StoreError};

impl Store {
    /// Checks the shared store's durable importer marker.
    ///
    /// # Errors
    /// Returns shared lock or SQLite errors.
    pub fn import_is_imported(&self, tool: &str, session_id: &str) -> Result<bool, StoreError> {
        let connection = self.activity_conn()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM import_state WHERE tool = ? AND session_id = ?",
            params![tool, session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Upserts a marker after the registry has persisted all facts/evidence.
    /// This method does not mark partially failed imports automatically.
    ///
    /// # Errors
    /// Returns shared lock or SQLite errors.
    pub fn import_mark_imported(
        &self,
        tool: &str,
        session_id: &str,
        memory_count: i64,
    ) -> Result<(), StoreError> {
        self.activity_conn()?.execute(
            "INSERT INTO import_state (tool,session_id,imported_at,memory_count) VALUES (?,?,?,?) ON CONFLICT(tool,session_id) DO UPDATE SET imported_at=excluded.imported_at,memory_count=excluded.memory_count",
            params![tool, session_id, crate::gotime::format(Utc::now()), memory_count],
        )?;
        Ok(())
    }

    /// Returns the latest stored marker; `None` represents Go's zero time.
    ///
    /// # Errors
    /// Returns shared lock, SQLite or timestamp decoding errors.
    pub fn import_last_time(&self, tool: &str) -> Result<Option<DateTime<Utc>>, StoreError> {
        let raw: rusqlite::types::Value = self.activity_conn()?.query_row(
            "SELECT MAX(imported_at) FROM import_state WHERE tool = ?",
            [tool],
            |row| row.get(0),
        )?;
        // MAX has no declared DATETIME type. The pinned sqlitekit DSN does
        // not enable _texttotime, so modernc returns a string, and NullTime's
        // Scan fails. Do not quietly turn that Go error into a usable cursor.
        let kind = match raw {
            rusqlite::types::Value::Null => return Ok(None),
            rusqlite::types::Value::Text(_) => "string",
            rusqlite::types::Value::Integer(_) => "int64",
            rusqlite::types::Value::Real(_) => "float64",
            rusqlite::types::Value::Blob(_) => "[]uint8",
        };
        Err(StoreError::Invalid(format!(
            "sql: Scan error on column index 0, name \"MAX(imported_at)\": unsupported Scan, storing driver.Value type {kind} into type *time.Time"
        )))
    }

    /// Counts durable sessions without discovering or reading source files.
    ///
    /// # Errors
    /// Returns shared lock or SQLite errors.
    pub fn import_count(&self, tool: &str) -> Result<i64, StoreError> {
        self.activity_conn()?
            .query_row(
                "SELECT COUNT(*) FROM import_state WHERE tool = ?",
                [tool],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}
