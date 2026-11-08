//! Working-memory eviction (ported for #760 from `internal/memory/working` at
//! v0.12.0).
//!
//! Working memories carry an `expires_at`. `compact_working_memories` removes
//! expired ones: a single expired memory is deleted outright; two or more are
//! first handed to a consolidation callback (the consolidation engine is not
//! ported yet, so the caller supplies it) and then evicted. The clock is
//! injected, and expiry is compared chronologically through
//! `symbrain_sync_time`, not as text.

use crate::gotime;
use crate::model::{Store, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

/// Outcome of a compaction (`working.CompactResult`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CompactResult {
    /// Expired, not yet archived working memories found.
    pub expired_count: i64,
    /// Rows deleted; zero on a dry run.
    pub evicted_count: i64,
}

const EXPIRED: &str = "tier = 'working' AND expires_at IS NOT NULL AND symbrain_sync_time(expires_at) <= symbrain_sync_time(?)";

impl Store {
    /// `GetExpiredWorkingMemories`: ids of working memories expired at `now`
    /// that are not archived, oldest expiry first.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn expired_working_memories(&self, now: DateTime<Utc>) -> Result<Vec<String>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(&format!(
            "SELECT id FROM memories WHERE {EXPIRED} AND consolidation_status != 'archived' ORDER BY symbrain_sync_time(expires_at) ASC, id ASC"
        ))?;
        let rows = statement.query_map([gotime::format(now)], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// `EvictExpiredWorkingMemories`: deletes expired working memories and
    /// returns how many rows went.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn evict_expired_working_memories(&self, now: DateTime<Utc>) -> Result<i64, StoreError> {
        let conn = self.lock()?;
        let deleted = conn.execute(
            &format!("DELETE FROM memories WHERE {EXPIRED}"),
            params![gotime::format(now)],
        )?;
        Ok(i64::try_from(deleted).unwrap_or(i64::MAX))
    }

    /// `Evictor.CompactWorkingMemories`. `consolidate` receives the ids of two
    /// or more expired memories before they are evicted (Go runs the
    /// consolidation engine there); its error aborts without evicting.
    ///
    /// # Errors
    /// A SQLite error or the callback's error.
    pub fn compact_working_memories(
        &self,
        now: DateTime<Utc>,
        dry_run: bool,
        consolidate: impl FnOnce(&[String]) -> Result<(), StoreError>,
    ) -> Result<CompactResult, StoreError> {
        let expired = self.expired_working_memories(now)?;
        let mut result = CompactResult {
            expired_count: i64::try_from(expired.len()).unwrap_or(i64::MAX),
            evicted_count: 0,
        };
        match expired.len() {
            0 => {}
            1 => {
                if !dry_run {
                    let conn = self.lock()?;
                    conn.execute("DELETE FROM memories WHERE id = ?", [&expired[0]])?;
                    result.evicted_count = 1;
                }
            }
            _ => {
                consolidate(&expired)?;
                if !dry_run {
                    result.evicted_count = self.evict_expired_working_memories(now)?;
                }
            }
        }
        Ok(result)
    }
}
