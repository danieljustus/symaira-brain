//! Cursor/oplog access uses the existing store and its existing triggers.

use super::{DeletedMemory, SyncMemory, SyncTime};
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, Row, params};

const LITE_COLUMNS: &str = "id,content,scope,metadata,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at";

impl Store {
    /// Reads a cursor under the literal remote string, without URL normalization.
    /// # Errors
    /// Returns a database or timestamp conversion error.
    pub fn sync_cursor(&self, remote: &str) -> Result<SyncTime, StoreError> {
        let raw: Option<String> = self
            .lock()?
            .query_row(
                "SELECT last_sync FROM sync_state WHERE remote=?",
                [remote],
                |row| row.get(0),
            )
            .optional()?;
        raw.map_or_else(|| Ok(SyncTime::default()), |raw| parse_time(&raw))
    }

    /// Persists the final cursor only after a successful whole run.
    /// # Errors
    /// Returns a database error.
    pub fn set_sync_cursor(&self, remote: &str, time: SyncTime) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO sync_state(remote,last_sync) VALUES(?,?) ON CONFLICT(remote) DO UPDATE SET last_sync=excluded.last_sync",
            params![remote, time.sqlite()]
        )?;
        Ok(())
    }

    /// Reads sync's lite projection: no public list visibility filtering,
    /// no retrieval feedback, entity hydration or embedding generation.
    /// # Errors
    /// Returns a database, row conversion or metadata decoding error.
    pub fn sync_memories_since(
        &self,
        since: SyncTime,
        last_id: &str,
        limit: i64,
    ) -> Result<Vec<SyncMemory>, StoreError> {
        let conn = self.lock()?;
        let predicate = if last_id.is_empty() {
            "updated_at>?"
        } else {
            "(updated_at>? OR(updated_at=? AND id>?))"
        };
        let query = format!(
            "SELECT {LITE_COLUMNS} FROM memories WHERE ({predicate}) AND COALESCE(json_extract(metadata,'$.sync_exclude'),'')!='true' ORDER BY updated_at ASC,id ASC LIMIT ?"
        );
        let mut statement = conn.prepare(&query)?;
        let stamp = since.sqlite();
        let limit = if limit <= 0 { 50_000 } else { limit };
        let mut rows = if last_id.is_empty() {
            statement.query(params![stamp, limit])?
        } else {
            statement.query(params![stamp, stamp, last_id, limit])?
        };
        let mut memories = Vec::new();
        while let Some(row) = rows.next()? {
            memories.push(scan(row)?);
        }
        Ok(memories)
    }

    /// Reads the frozen tombstone keyset, including its continuation behavior.
    /// The missing op predicate after the first page is an inherited Go defect,
    /// explicitly tracked in the sync ADR, not silently corrected here.
    /// # Errors
    /// Returns a database or timestamp conversion error.
    pub fn sync_deleted_since(
        &self,
        since: SyncTime,
        last_id: &str,
        limit: i64,
    ) -> Result<Vec<DeletedMemory>, StoreError> {
        let conn = self.lock()?;
        let predicate = if last_id.is_empty() {
            "o.op='delete' AND o.ts>?"
        } else {
            "(o.ts>? OR(o.ts=? AND o.memory_id>?))"
        };
        let query = format!(
            "SELECT o.memory_id,o.ts FROM sync_oplog o WHERE {predicate} AND o.event_id=(SELECT MAX(event_id) FROM sync_oplog WHERE memory_id=o.memory_id) ORDER BY o.ts ASC,o.memory_id ASC LIMIT ?"
        );
        let mut statement = conn.prepare(&query)?;
        let stamp = since.sqlite();
        let limit = if limit <= 0 { 50_000 } else { limit };
        let mut rows = if last_id.is_empty() {
            statement.query(params![stamp, limit])?
        } else {
            statement.query(params![stamp, stamp, last_id, limit])?
        };
        let mut deleted = Vec::new();
        while let Some(row) = rows.next()? {
            deleted.push(DeletedMemory {
                id: row.get(0)?,
                deleted_at: parse_time(&row.get::<_, String>(1)?)?,
            });
        }
        Ok(deleted)
    }

    /// Deletes on equality, without the CLI service's retrieval-feedback step.
    /// # Errors
    /// Returns a database or timestamp conversion error. Audit failures are
    /// ignored, exactly as the storage-layer DeleteMemory contract specifies.
    pub fn apply_sync_delete(&self, deleted: &DeletedMemory) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM memories WHERE id=?",
                [&deleted.id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(false);
        };
        if parse_time(&raw)? > deleted.deleted_at {
            return Ok(false);
        }
        let identity: Option<(Option<String>, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT scope,created_by,created_session FROM memories WHERE id=?",
                [&deleted.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((scope, author, session)) = identity {
            conn.execute("DELETE FROM memories WHERE id=?", [&deleted.id])?;
            crate::cli_write::audit(
                &conn,
                "delete",
                &deleted.id,
                &scope.unwrap_or_default(),
                &session.unwrap_or_default(),
                &author.unwrap_or_default(),
                "",
            );
        }
        Ok(true)
    }
}

pub(super) fn parse_time(raw: &str) -> Result<SyncTime, StoreError> {
    SyncTime::parse_sqlite(raw)
        .ok_or_else(|| StoreError::Invalid(format!("invalid sync timestamp: {raw}")))
}

fn optional_time(row: &Row<'_>, index: usize) -> Result<Option<SyncTime>, StoreError> {
    row.get::<_, Option<String>>(index)?
        .map(|raw| parse_time(&raw))
        .transpose()
}

fn scan(row: &Row<'_>) -> Result<SyncMemory, StoreError> {
    let raw: String = row.get(3)?;
    Ok(SyncMemory {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        metadata: serde_json::from_str(&raw)
            .map_err(|error| StoreError::Invalid(error.to_string()))?,
        created_at: parse_time(&row.get::<_, String>(4)?)?,
        updated_at: parse_time(&row.get::<_, String>(5)?)?,
        created_by: row.get(6)?,
        updated_by: row.get(7)?,
        created_session: row.get(8)?,
        updated_session: row.get(9)?,
        consolidation_status: row.get(10)?,
        consolidated_into_id: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        importance: row.get(12)?,
        valid_from: optional_time(row, 13)?,
        valid_to: optional_time(row, 14)?,
        superseded_by: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
        tier: row.get(16)?,
        expires_at: optional_time(row, 17)?,
        access_count: row.get(18)?,
        last_access: optional_time(row, 19)?,
        prev_access: optional_time(row, 20)?,
        review_status: row.get(21)?,
        kind: row.get(22)?,
        decay_factor: row.get(23)?,
        retired_at: optional_time(row, 24)?,
        ..SyncMemory::default()
    })
}
