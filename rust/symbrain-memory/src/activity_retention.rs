//! Activity retention and clearing (ported for #761 from
//! `internal/memory/activity/retention.go`).
//!
//! Every deletion is transactional and then enables `secure_delete`,
//! checkpoints and truncates the WAL, vacuums and re-checkpoints, so a delete
//! is an on-disk erasure and not merely a hidden row. Durable memories,
//! evidence and other tables are never touched.

use crate::activity_episode::episode_by_id;
use crate::gotime;
use crate::model::{Store, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

/// Rows removed by an expiry pass or an explicit clear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionResult {
    pub segments: usize,
    pub episodes: usize,
}

fn invalid(message: impl Into<String>) -> StoreError {
    StoreError::Invalid(message.into())
}

/// `finalizeDeletion` + `VerifyDiskClean`.
fn finalize(conn: &Connection) -> Result<(), StoreError> {
    let checkpoint = |label: &str| -> Result<(), StoreError> {
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(|error| invalid(format!("{label}: {error}")))
    };
    conn.pragma_update(None, "secure_delete", "ON")
        .map_err(|error| invalid(format!("enable secure activity deletion: {error}")))?;
    checkpoint("checkpoint activity WAL")?;
    conn.execute_batch("VACUUM")
        .map_err(|error| invalid(format!("vacuum activity database: {error}")))?;
    checkpoint("checkpoint activity WAL after vacuum")?;
    verify_disk_clean(conn)
}

/// A completed deletion must leave no pending WAL pages.
fn verify_disk_clean(conn: &Connection) -> Result<(), StoreError> {
    let Some(path) = conn.path().filter(|path| !path.is_empty()) else {
        return Ok(());
    };
    match std::fs::metadata(format!("{path}-wal")) {
        Ok(info) if info.len() != 0 => Err(invalid(format!(
            "activity deletion left {} bytes in SQLite WAL",
            info.len()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(invalid(format!("inspect SQLite WAL: {error}"))),
    }
}

fn verify_absent(conn: &Connection, ids: &[String]) -> Result<(), StoreError> {
    for id in ids {
        for (table, noun) in [
            ("activity_segments", "segment"),
            ("activity_episodes", "episode"),
        ] {
            let count: i64 = conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE id = ?"),
                [id],
                |row| row.get(0),
            )?;
            if count != 0 {
                return Err(invalid(format!(
                    "activity {noun} {id} still exists after deletion"
                )));
            }
        }
    }
    Ok(())
}

impl Store {
    /// `Expire` / `PurgeExpired`: deletes rows whose expiry is at or before `now`.
    ///
    /// # Errors
    /// A SQLite error or a failed disk-clean check.
    pub fn expire_activity(&self, now: DateTime<Utc>) -> Result<RetentionResult, StoreError> {
        let conn = self.lock()?;
        let stamp = gotime::format(now);
        let transaction = conn.unchecked_transaction()?;
        let episodes = transaction.execute(
            "DELETE FROM activity_episodes WHERE expires_at <= ?",
            [&stamp],
        )?;
        let segments = transaction.execute(
            "DELETE FROM activity_segments WHERE expires_at <= ?",
            [&stamp],
        )?;
        transaction.commit()?;
        if segments > 0 || episodes > 0 {
            finalize(&conn)?;
        }
        Ok(RetentionResult { segments, episodes })
    }

    /// `DeleteEpisode` / `ClearEpisode`: removes an episode and every segment
    /// its provenance names, then verifies the ids are gone. A missing episode
    /// is a no-op.
    ///
    /// # Errors
    /// A SQLite error or a failed verification.
    pub fn delete_activity_episode(&self, id: &str) -> Result<RetentionResult, StoreError> {
        let conn = self.lock()?;
        let Some(episode) = episode_by_id(&conn, id)? else {
            return Ok(RetentionResult::default());
        };
        let transaction = conn.unchecked_transaction()?;
        let mut result = RetentionResult {
            segments: 0,
            episodes: transaction.execute("DELETE FROM activity_episodes WHERE id = ?", [id])?,
        };
        for segment_id in &episode.segment_ids {
            result.segments +=
                transaction.execute("DELETE FROM activity_segments WHERE id = ?", [segment_id])?;
        }
        transaction.commit()?;
        finalize(&conn)?;
        let mut ids = vec![id.to_owned()];
        ids.extend(episode.segment_ids);
        verify_absent(&conn, &ids)?;
        Ok(result)
    }

    /// `ClearTimeRange`: removes every episode and segment overlapping
    /// `[start, end)`, episodes first.
    ///
    /// # Errors
    /// A non-increasing range or a SQLite error.
    pub fn clear_activity_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<RetentionResult, StoreError> {
        if end <= start {
            return Err(invalid("activity clear range must be increasing"));
        }
        let conn = self.lock()?;
        let transaction = conn.unchecked_transaction()?;
        let bounds = params![gotime::format(start), gotime::format(end)];
        let episodes = transaction.execute(
            "DELETE FROM activity_episodes WHERE ended_at > ? AND started_at < ?",
            bounds,
        )?;
        let segments = transaction.execute(
            "DELETE FROM activity_segments WHERE ended_at > ? AND started_at < ?",
            bounds,
        )?;
        transaction.commit()?;
        finalize(&conn)?;
        Ok(RetentionResult { segments, episodes })
    }

    /// `ClearAll` / `ClearAllActivity`.
    ///
    /// # Errors
    /// A SQLite error or a failed disk-clean check.
    pub fn clear_all_activity(&self) -> Result<RetentionResult, StoreError> {
        let conn = self.lock()?;
        let transaction = conn.unchecked_transaction()?;
        let episodes = transaction.execute("DELETE FROM activity_episodes", [])?;
        let segments = transaction.execute("DELETE FROM activity_segments", [])?;
        transaction.commit()?;
        finalize(&conn)?;
        Ok(RetentionResult { segments, episodes })
    }

    /// `ClearLastSession`: deletes the latest episode (by end time, then id)
    /// and its segment provenance; no episode is a successful no-op.
    ///
    /// # Errors
    /// As [`Store::delete_activity_episode`].
    pub fn clear_last_activity_session(&self) -> Result<RetentionResult, StoreError> {
        let latest: Option<String> = {
            let conn = self.lock()?;
            conn.query_row(
                "SELECT id FROM activity_episodes ORDER BY ended_at DESC, id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?
        };
        latest.map_or_else(
            || Ok(RetentionResult::default()),
            |id| self.delete_activity_episode(&id),
        )
    }
}
