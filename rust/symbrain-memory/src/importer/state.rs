//! Import bookkeeping (ported from `internal/memory/importer/state.go`): which
//! sessions were imported already, kept in the `import_state` table.

use crate::gotime;
use crate::model::{Store, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::params;

impl Store {
    /// `IsImported`.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn is_session_imported(&self, tool: &str, session_id: &str) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM import_state WHERE tool = ? AND session_id = ?",
            params![tool, session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// `MarkImported`: upserts the session with its fact count.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn mark_session_imported(
        &self,
        tool: &str,
        session_id: &str,
        memory_count: usize,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO import_state (tool, session_id, imported_at, memory_count) VALUES (?, ?, ?, ?) ON CONFLICT(tool, session_id) DO UPDATE SET imported_at = excluded.imported_at, memory_count = excluded.memory_count",
            params![
                tool,
                session_id,
                gotime::format(now),
                i64::try_from(memory_count).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// `GetLastImportTime`: newest import of a tool, `None` if never.
    ///
    /// # Errors
    /// A SQLite error or an unreadable stored timestamp.
    pub fn last_import_time(&self, tool: &str) -> Result<Option<DateTime<Utc>>, StoreError> {
        let conn = self.lock()?;
        let rows = {
            let mut statement =
                conn.prepare("SELECT imported_at FROM import_state WHERE tool = ?")?;
            let rows = statement.query_map([tool], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut newest: Option<DateTime<Utc>> = None;
        for raw in rows {
            let parsed = gotime::parse(&raw).ok_or_else(|| {
                StoreError::Invalid(format!("unparsable import_state.imported_at {raw:?}"))
            })?;
            newest = newest.max(Some(parsed));
        }
        Ok(newest)
    }

    /// `GetImportCount`: imported sessions of a tool.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn import_count(&self, tool: &str) -> Result<usize, StoreError> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM import_state WHERE tool = ?",
            [tool],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn state_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 1, 12, 30, 0).unwrap();
        assert!(!store.is_session_imported("git", "s1").unwrap());
        assert_eq!(store.last_import_time("git").unwrap(), None);
        store.mark_session_imported("git", "s1", 3, t1).unwrap();
        store.mark_session_imported("git", "s2", 1, t2).unwrap();
        store.mark_session_imported("hermes", "s1", 9, t2).unwrap();
        // Re-marking is an upsert, not a duplicate.
        store.mark_session_imported("git", "s1", 4, t1).unwrap();
        assert!(store.is_session_imported("git", "s1").unwrap());
        assert!(!store.is_session_imported("codex", "s1").unwrap());
        assert_eq!(store.import_count("git").unwrap(), 2);
        assert_eq!(store.import_count("hermes").unwrap(), 1);
        assert_eq!(store.last_import_time("git").unwrap(), Some(t2));
        let count: i64 = store
            .lock()
            .unwrap()
            .query_row(
                "SELECT memory_count FROM import_state WHERE tool='git' AND session_id='s1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4);
    }
}
