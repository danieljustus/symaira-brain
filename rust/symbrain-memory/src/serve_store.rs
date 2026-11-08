//! Store operations behind the sync, relay and stats routes of `memory serve`.
//!
//! These mirror `db.GetMemoriesSinceCursor`, `GetDeletedSince`,
//! `SyncUpsertMemoryIfNewer` (with vectors), `StoreRelayBlob` and
//! `GetRelayBlobsSince`. Timestamps use the same driver rendering as the
//! rest of the native store so a database written by either implementation
//! is read and written identically.

use crate::cli_write::{audit, embedding_json, sign_bits};
use crate::list_rows::{LITE_COLUMNS, MemoryListRow, scan_lite};
use crate::model::{Store, StoreError};
use crate::search_rows::{SEARCH_COLUMNS, SearchRow};
use crate::sync_apply::{
    PulledDelete, PulledMemory, apply_remote_delete, non_null, optional_time, stored_time,
};
use crate::sync_state::render_cursor;
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

/// One memory of a changes reply: lite (no vector) or full.
pub(crate) enum SyncRow {
    Lite(MemoryListRow),
    Full(SearchRow),
}

impl SyncRow {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Lite(row) => &row.id,
            Self::Full(row) => &row.id,
        }
    }

    pub(crate) fn updated_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Lite(row) => row.updated_at.map(|time| time.with_timezone(&Utc)),
            Self::Full(row) => row.updated_at.map(|time| time.with_timezone(&Utc)),
        }
    }

    pub(crate) fn metadata(&self) -> &serde_json::Map<String, serde_json::Value> {
        match self {
            Self::Lite(row) => &row.metadata,
            Self::Full(row) => &row.metadata,
        }
    }

    pub(crate) fn redact_pii(&mut self) {
        match self {
            Self::Lite(row) => row.redact_pii(),
            Self::Full(row) => row.redact_pii(),
        }
    }

    pub(crate) fn to_json(&self) -> String {
        match self {
            Self::Lite(row) => row.to_go_json(),
            Self::Full(row) => row.to_go_json_embedded(),
        }
    }
}

/// One relay blob (`db.RelayBlob`).
pub(crate) struct RelayBlob {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub blob: Vec<u8>,
}

/// What the apply route needs to know about the runtime configuration.
#[derive(Clone, Copy)]
pub(crate) struct ApplyMode {
    pub quantize_binary: bool,
    pub audit_enabled: bool,
}

impl Store {
    /// `GetMemoriesSinceCursor`.
    pub(crate) fn sync_changes(
        &self,
        since: DateTime<Utc>,
        limit: i64,
        full: bool,
    ) -> Result<Vec<SyncRow>, StoreError> {
        let conn = self.lock()?;
        let columns = if full { SEARCH_COLUMNS } else { LITE_COLUMNS };
        let sql = format!(
            "SELECT {columns} FROM memories WHERE symbrain_sync_time(updated_at) > symbrain_sync_time(?) ORDER BY symbrain_sync_time(updated_at) ASC LIMIT ?"
        );
        let mut statement = conn.prepare(&sql)?;
        let mut rows = statement.query(params![render_cursor(since), limit])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(if full {
                SyncRow::Full(SearchRow::from_row(row)?)
            } else {
                SyncRow::Lite(scan_lite(row)?)
            });
        }
        Ok(result)
    }

    /// `GetDeletedSince`: tombstones whose latest oplog entry is a delete.
    pub(crate) fn deleted_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<(String, DateTime<Utc>)>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT o.memory_id, o.ts FROM sync_oplog o WHERE o.op = 'delete' AND symbrain_sync_time(o.ts) > symbrain_sync_time(?) AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)",
        )?;
        let mut rows = statement.query([render_cursor(since)])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let ts: Value = row.get(1)?;
            let at = stored_time(&ts)
                .ok_or_else(|| StoreError::Invalid("unrecognized oplog timestamp".to_owned()))?;
            result.push((id, at));
        }
        Ok(result)
    }

    /// `ApplyRemoteDelete`; `Err` is a database failure (HTTP 500).
    pub(crate) fn serve_apply_delete(
        &self,
        id: &str,
        deleted_at: DateTime<Utc>,
        audit_enabled: bool,
    ) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let transaction = conn.unchecked_transaction()?;
        let delete = PulledDelete {
            id: id.to_owned(),
            deleted_at,
        };
        let removed = apply_remote_delete(&transaction, &delete, audit_enabled)
            .ok_or_else(|| StoreError::Invalid("apply remote delete failed".to_owned()))?;
        transaction.commit()?;
        Ok(removed)
    }

    /// `SyncUpsertMemoryIfNewer` with the stored vector, LSH bucket and
    /// optional sign-bit binary; `Ok(true)` when the row was written.
    pub(crate) fn serve_sync_upsert(
        &self,
        memory: &PulledMemory,
        mode: ApplyMode,
    ) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let latest: Option<(String, Value)> = conn
            .query_row(
                "SELECT op, ts FROM sync_oplog WHERE memory_id = ? ORDER BY event_id DESC LIMIT 1",
                [&memory.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((op, ts)) = latest {
            let ts = stored_time(&ts)
                .ok_or_else(|| StoreError::Invalid("unrecognized oplog timestamp".to_owned()))?;
            if op == "delete" && memory.updated_at <= ts {
                return Ok(false);
            }
        }
        upsert(&conn, memory, mode)
    }

    /// `StoreRelayBlob`: last-writer-wins on `updated_at`.
    pub(crate) fn store_relay_blob(&self, blob: &RelayBlob) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let existing: Option<Value> = conn
            .query_row(
                "SELECT updated_at FROM sync_relay WHERE id = ?",
                [&blob.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let existing = stored_time(&existing)
                .ok_or_else(|| StoreError::Invalid("unrecognized relay timestamp".to_owned()))?;
            if blob.updated_at <= existing {
                return Ok(false);
            }
        }
        conn.execute(
            "INSERT INTO sync_relay (id, updated_at, blob) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at, blob = excluded.blob",
            params![blob.id, render_cursor(blob.updated_at), blob.blob],
        )?;
        Ok(true)
    }

    /// `GetRelayBlobsSince`.
    pub(crate) fn relay_blobs_since(
        &self,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<RelayBlob>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id, updated_at, blob FROM sync_relay WHERE symbrain_sync_time(updated_at) > symbrain_sync_time(?) ORDER BY symbrain_sync_time(updated_at) ASC LIMIT ?",
        )?;
        let mut rows = statement.query(params![render_cursor(since), limit])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let at: Value = row.get(1)?;
            result.push(RelayBlob {
                id: row.get(0)?,
                updated_at: stored_time(&at).ok_or_else(|| {
                    StoreError::Invalid("unrecognized relay timestamp".to_owned())
                })?,
                blob: row.get(2)?,
            });
        }
        Ok(result)
    }
}

#[allow(clippy::too_many_lines)] // One linear pass mirroring the shipped upsert.
fn upsert(conn: &Connection, memory: &PulledMemory, mode: ApplyMode) -> Result<bool, StoreError> {
    let vector = memory.embedding.clone().unwrap_or_default();
    let embedding = match &memory.embedding {
        None => "null".to_owned(),
        Some(vector) if vector.is_empty() => "[]".to_owned(),
        Some(vector) => embedding_json(vector),
    };
    let lsh = crate::lsh::compute_lsh(&vector).map_err(StoreError::Invalid)?;
    let binary = (mode.quantize_binary && !vector.is_empty()).then(|| sign_bits(&vector));
    let dimension = i64::try_from(vector.len()).unwrap_or(0);
    let metadata = crate::cli_write::go_json(&memory.metadata)?;
    let content_hash = if memory.content_hash.is_empty() {
        crate::write::content_hash(&memory.content)
    } else {
        memory.content_hash.clone()
    };
    let default =
        |value: &str, fallback: &str| if value.is_empty() { fallback } else { value }.to_owned();
    let status = default(&memory.consolidation_status, "raw");
    let review = default(&memory.review_status, "approved");
    let decay = if memory.decay_factor <= 0.0 || memory.decay_factor > 1.0 {
        1.0
    } else {
        memory.decay_factor
    };
    let existing: Option<Value> = conn
        .query_row(
            "SELECT updated_at FROM memories WHERE id = ?",
            [&memory.id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(existing) = existing else {
        conn.execute(
            "INSERT INTO memories (id, content, scope, metadata, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                memory.id, memory.content, memory.scope, metadata, embedding, binary, dimension,
                memory.embedding_source, memory.embedding_model, memory.embedding_quantization,
                content_hash, lsh, render_cursor(memory.created_at), render_cursor(memory.updated_at),
                memory.created_by, memory.updated_by, memory.created_session, memory.updated_session,
                status, non_null(&memory.consolidated_into_id), memory.importance,
                optional_time(memory.valid_from), optional_time(memory.valid_to),
                non_null(&memory.superseded_by), memory.tier, optional_time(memory.expires_at),
                memory.access_count, optional_time(memory.last_access),
                optional_time(memory.prev_access), review, memory.kind, decay,
                optional_time(memory.retired_at),
            ],
        )?;
        if mode.audit_enabled {
            audit(
                conn,
                "set",
                &memory.id,
                &memory.scope,
                &memory.created_session,
                &memory.created_by,
                "",
            );
        }
        return Ok(true);
    };
    let existing = stored_time(&existing)
        .ok_or_else(|| StoreError::Invalid("unrecognized memory timestamp".to_owned()))?;
    if memory.updated_at <= existing {
        if memory.updated_at < existing {
            return Ok(false);
        }
        return reconcile_equal(conn, memory, mode).map_err(Into::into);
    }
    let columns = if vector.is_empty() {
        crate::sync_embedding::preserved(conn, &memory.id, &memory.content)?
    } else {
        None
    }
    .unwrap_or_else(|| {
        crate::sync_embedding::EmbeddingColumns::incoming(
            embedding.clone(),
            binary.clone(),
            dimension,
            [
                &memory.embedding_source,
                &memory.embedding_model,
                &memory.embedding_quantization,
            ],
            lsh,
        )
    });
    conn.execute(
        "UPDATE memories SET content=?, scope=?, metadata=?, embedding=?, embedding_binary=?, embedding_dim=?, embedding_source=?, embedding_model=?, embedding_quantization=?, content_hash=?, lsh_hash=?, updated_at=?, updated_by=?, updated_session=?, consolidation_status=?, consolidated_into_id=?, valid_from=?, valid_to=?, superseded_by=?, importance=?, tier=?, expires_at=?, access_count=?, last_access=?, prev_access=?, review_status=?, kind=?, decay_factor=?, retired_at=? WHERE id=?",
        params![
            memory.content, memory.scope, metadata, columns.embedding, columns.binary,
            columns.dimension, columns.source, columns.model, columns.quantization,
            content_hash, columns.lsh, render_cursor(memory.updated_at), memory.updated_by,
            memory.updated_session, status, non_null(&memory.consolidated_into_id),
            optional_time(memory.valid_from), optional_time(memory.valid_to),
            non_null(&memory.superseded_by), memory.importance, memory.tier, optional_time(memory.expires_at), memory.access_count,
            optional_time(memory.last_access), optional_time(memory.prev_access), review,
            memory.kind, decay, optional_time(memory.retired_at), memory.id,
        ],
    )?;
    if mode.audit_enabled {
        audit(
            conn,
            "update",
            &memory.id,
            &memory.scope,
            &memory.updated_session,
            &memory.updated_by,
            "",
        );
    }
    Ok(true)
}

/// Equal `updated_at` on both peers: keep the greater lifecycle (#855).
fn reconcile_equal(
    conn: &Connection,
    memory: &PulledMemory,
    mode: ApplyMode,
) -> rusqlite::Result<bool> {
    let incoming = crate::sync_lifecycle::Lifecycle {
        retired_at: optional_time(memory.retired_at),
        superseded_by: memory.superseded_by.clone(),
        consolidated_into_id: memory.consolidated_into_id.clone(),
        valid_from: optional_time(memory.valid_from),
        valid_to: optional_time(memory.valid_to),
    };
    let audited = crate::sync_lifecycle::Audited {
        enabled: mode.audit_enabled,
        scope: &memory.scope,
        session: &memory.updated_session,
        actor: &memory.updated_by,
    };
    crate::sync_lifecycle::reconcile_equal(conn, &memory.id, &incoming, &audited)
}
