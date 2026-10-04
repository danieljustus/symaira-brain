//! Shared-store activity retention. Counts survive post-commit cleanup errors,
//! just as the Go API returns a RetentionResult together with an error.
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{Store, StoreError};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActivityRetentionResult {
    pub segments: usize,
    pub episodes: usize,
}

#[derive(Debug)]
pub struct ActivityDeletion {
    pub deleted: ActivityRetentionResult,
    pub error: Option<StoreError>,
}

impl ActivityDeletion {
    fn failed(error: StoreError) -> Self {
        Self {
            deleted: ActivityRetentionResult::default(),
            error: Some(error),
        }
    }
}

impl Store {
    /// Expires rows at the supplied instant, or the current UTC time. Cleanup
    /// is skipped when no rows were deleted, matching the frozen owner.
    #[must_use]
    pub fn activity_expire(&self, at: Option<DateTime<Utc>>) -> ActivityDeletion {
        let at = at.filter(|time| !zero(*time)).unwrap_or_else(Utc::now);
        self.activity_delete_rows(
            "DELETE FROM activity_episodes WHERE expires_at <= ?",
            "DELETE FROM activity_segments WHERE expires_at <= ?",
            &[crate::gotime::format(at)],
            false,
        )
    }

    /// Clears overlapping rows in [start, end), episodes first. Durable
    /// memories and grounded evidence are never touched by this operation.
    #[must_use]
    pub fn activity_clear_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ActivityDeletion {
        if zero(end) || end <= start {
            return ActivityDeletion::failed(StoreError::Invalid(
                "activity clear range must be increasing".into(),
            ));
        }
        self.activity_delete_rows(
            "DELETE FROM activity_episodes WHERE ended_at > ? AND started_at < ?",
            "DELETE FROM activity_segments WHERE ended_at > ? AND started_at < ?",
            &[crate::gotime::format(start), crate::gotime::format(end)],
            true,
        )
    }

    /// Clears only activity tables, then checkpoints and vacuums the shared
    /// database. Cleanup also runs on an already-empty activity store.
    #[must_use]
    pub fn activity_clear_all(&self) -> ActivityDeletion {
        self.activity_delete_rows(
            "DELETE FROM activity_episodes",
            "DELETE FROM activity_segments",
            &[],
            true,
        )
    }

    fn activity_delete_rows(
        &self,
        episode_sql: &str,
        segment_sql: &str,
        arguments: &[String],
        always_cleanup: bool,
    ) -> ActivityDeletion {
        let mut connection = match self.activity_conn() {
            Ok(connection) => connection,
            Err(error) => return ActivityDeletion::failed(error),
        };
        let transaction = match connection.transaction() {
            Ok(transaction) => transaction,
            Err(error) => return ActivityDeletion::failed(error.into()),
        };
        let result = (|| -> Result<ActivityRetentionResult, StoreError> {
            let episodes =
                transaction.execute(episode_sql, rusqlite::params_from_iter(arguments))?;
            let segments =
                transaction.execute(segment_sql, rusqlite::params_from_iter(arguments))?;
            transaction.commit()?;
            Ok(ActivityRetentionResult { segments, episodes })
        })();
        let deleted = match result {
            Ok(deleted) => deleted,
            Err(error) => return ActivityDeletion::failed(error),
        };
        let error = if always_cleanup || deleted.segments > 0 || deleted.episodes > 0 {
            finalize(&connection).err()
        } else {
            None
        };
        ActivityDeletion { deleted, error }
    }
}

fn zero(time: DateTime<Utc>) -> bool {
    time.timestamp() == -62_135_596_800 && time.timestamp_subsec_nanos() == 0
}

fn finalize(connection: &Connection) -> Result<(), StoreError> {
    for (statement, context) in [
        (
            "PRAGMA secure_delete = ON",
            "enable secure activity deletion",
        ),
        ("PRAGMA wal_checkpoint(TRUNCATE)", "checkpoint activity WAL"),
        ("VACUUM", "vacuum activity database"),
        (
            "PRAGMA wal_checkpoint(TRUNCATE)",
            "checkpoint activity WAL after vacuum",
        ),
    ] {
        connection
            .execute_batch(statement)
            .map_err(|error| StoreError::Invalid(format!("{context}: {error}")))?;
    }
    // database_list reads the path of this exact existing connection. It does
    // not open a second store and does not guess the configured filename.
    let path: String = connection.query_row(
        "SELECT file FROM pragma_database_list WHERE name = ?",
        params!["main"],
        |row| row.get(0),
    )?;
    if path.is_empty() {
        return Ok(());
    }
    match std::fs::metadata(format!("{path}-wal")) {
        Ok(info) if info.len() != 0 => Err(StoreError::Invalid(format!(
            "activity deletion left {} bytes in SQLite WAL",
            info.len()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Invalid(format!("inspect SQLite WAL: {error}"))),
    }
}

#[cfg(test)]
#[path = "activity_retention_tests.rs"]
mod tests;
