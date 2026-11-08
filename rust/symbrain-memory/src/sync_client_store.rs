//! The store half of the native `memory sync` client: the per-remote cursor,
//! the local change streams that are pushed, and the tombstone-aware
//! last-writer-wins application of pulled changes.
//!
//! Every operation mirrors the shipped `db` method it is named after, including
//! the rendering of bound timestamps (`Time.String()` in the zone the
//! timestamp carries), the scan rules of the driver and its error wording.
//! Errors are the texts Go would print.

use crate::cli_write::{audit, embedding_json, sign_bits};
use crate::model::Store;
use crate::sync_client_json::{ClientMemory, LocalMemory, quote};
use crate::sync_client_time::{GoTime, parse_stored};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::BTreeMap;

/// Runtime switches of the shipped `db.Open` that the apply path honours.
#[derive(Clone, Copy, Debug)]
pub struct ClientApplyMode {
    /// `hybrid_search.quantize_to_binary`.
    pub quantize_binary: bool,
    /// `audit_log_enabled`.
    pub audit_enabled: bool,
}

fn scan_error(index: usize, name: &str, detail: &str) -> String {
    format!("sql: Scan error on column index {index}, name \"{name}\": {detail}")
}

fn driver_type(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "<nil>",
        ValueRef::Integer(_) => "int64",
        ValueRef::Real(_) => "float64",
        ValueRef::Text(_) => "string",
        ValueRef::Blob(_) => "[]uint8",
    }
}

fn time_value(value: ValueRef<'_>, index: usize, name: &str) -> Result<GoTime, String> {
    if let ValueRef::Text(bytes) = value
        && let Some(time) = parse_stored(&String::from_utf8_lossy(bytes))
    {
        return Ok(time);
    }
    Err(scan_error(
        index,
        name,
        &format!(
            "unsupported Scan, storing driver.Value type {} into type *time.Time",
            driver_type(value)
        ),
    ))
}

fn time_col(row: &Row<'_>, index: usize, name: &str) -> Result<GoTime, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    time_value(value, index, name)
}

fn opt_time_col(row: &Row<'_>, index: usize, name: &str) -> Result<Option<GoTime>, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    if matches!(value, ValueRef::Null) {
        return Ok(None);
    }
    time_value(value, index, name).map(Some)
}

fn text_col(row: &Row<'_>, index: usize, name: &str) -> Result<String, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    match value {
        ValueRef::Null => Err(scan_error(
            index,
            name,
            "converting NULL to string is unsupported",
        )),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            Ok(String::from_utf8_lossy(bytes).into_owned())
        }
        ValueRef::Integer(number) => Ok(number.to_string()),
        ValueRef::Real(number) => Ok(format!("{number}")),
    }
}

fn opt_text_col(row: &Row<'_>, index: usize) -> Result<String, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    Ok(match value {
        ValueRef::Null => String::new(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
        ValueRef::Integer(number) => number.to_string(),
        ValueRef::Real(number) => format!("{number}"),
    })
}

fn float_col(row: &Row<'_>, index: usize, name: &str) -> Result<f64, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    match value {
        ValueRef::Real(number) => Ok(number),
        #[allow(clippy::cast_precision_loss)]
        ValueRef::Integer(number) => Ok(number as f64),
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            text.parse::<f64>().map_err(|_| {
                scan_error(
                    index,
                    name,
                    &format!(
                        "converting driver.Value type string ({}) to a float64: invalid syntax",
                        crate::sync_client_scan::go_quote(&text)
                    ),
                )
            })
        }
        ValueRef::Null => Err(scan_error(
            index,
            name,
            "converting NULL to float64 is unsupported",
        )),
        ValueRef::Blob(_) => Err(scan_error(
            index,
            name,
            "converting driver.Value type []uint8 to a float64: invalid syntax",
        )),
    }
}

fn int_col(row: &Row<'_>, index: usize, name: &str) -> Result<i64, String> {
    let value = row.get_ref(index).map_err(|error| error.to_string())?;
    match value {
        ValueRef::Integer(number) => Ok(number),
        ValueRef::Null => Err(scan_error(
            index,
            name,
            "converting NULL to int64 is unsupported",
        )),
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            text.parse::<i64>().map_err(|_| {
                scan_error(
                    index,
                    name,
                    &format!(
                        "converting driver.Value type string ({}) to a int64: invalid syntax",
                        crate::sync_client_scan::go_quote(&text)
                    ),
                )
            })
        }
        ValueRef::Real(number) => Err(scan_error(
            index,
            name,
            &format!(
                "converting driver.Value type float64 (\"{number}\") to a int64: invalid syntax"
            ),
        )),
        ValueRef::Blob(_) => Err(scan_error(
            index,
            name,
            "converting driver.Value type []uint8 to a int64: invalid syntax",
        )),
    }
}

fn sql(error: &impl std::fmt::Display) -> String {
    error.to_string()
}

fn metadata_json(metadata: Option<&BTreeMap<String, String>>) -> String {
    metadata.map_or_else(
        || "null".to_owned(),
        |map| {
            let items: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
                .collect();
            format!("{{{}}}", items.join(","))
        },
    )
}

/// Columns of the shipped lite projection.
const LITE: &str = "id, content, scope, metadata, created_at, updated_at, created_by, \
     updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, \
     importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, \
     prev_access, review_status, kind, decay_factor, retired_at";

fn scan_lite(row: &Row<'_>) -> Result<LocalMemory, String> {
    let metadata_text = text_col(row, 3, "metadata")?;
    let metadata = crate::sync_client_json::decode_metadata(&metadata_text)?;
    Ok(LocalMemory {
        id: text_col(row, 0, "id")?,
        content: text_col(row, 1, "content")?,
        scope: text_col(row, 2, "scope")?,
        metadata,
        created_at: time_col(row, 4, "created_at")?,
        updated_at: time_col(row, 5, "updated_at")?,
        created_by: text_col(row, 6, "created_by")?,
        updated_by: text_col(row, 7, "updated_by")?,
        created_session: text_col(row, 8, "created_session")?,
        updated_session: text_col(row, 9, "updated_session")?,
        consolidation_status: text_col(row, 10, "consolidation_status")?,
        consolidated_into_id: opt_text_col(row, 11)?,
        importance: float_col(row, 12, "importance")?,
        valid_from: opt_time_col(row, 13, "valid_from")?,
        valid_to: opt_time_col(row, 14, "valid_to")?,
        superseded_by: opt_text_col(row, 15)?,
        tier: text_col(row, 16, "tier")?,
        expires_at: opt_time_col(row, 17, "expires_at")?,
        access_count: int_col(row, 18, "access_count")?,
        last_access: opt_time_col(row, 19, "last_access")?,
        prev_access: opt_time_col(row, 20, "prev_access")?,
        review_status: text_col(row, 21, "review_status")?,
        kind: text_col(row, 22, "kind")?,
        decay_factor: float_col(row, 23, "decay_factor")?,
        retired_at: opt_time_col(row, 24, "retired_at")?,
        embedding: serde_json::from_str(&opt_text_col(row, 25)?).unwrap_or_default(),
        embedding_source: opt_text_col(row, 26)?,
        embedding_model: opt_text_col(row, 27)?,
        embedding_quantization: opt_text_col(row, 28)?,
        extras: crate::sync_graph::MemoryExtras::default(),
    })
}

fn bound(time: &GoTime) -> String {
    time.to_go_string()
}

fn optional_bound(time: Option<&GoTime>) -> Option<String> {
    time.map(bound)
}

fn non_null(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

impl Store {
    /// `GetSyncCursor`: the zero time when the remote has no row.
    ///
    /// # Errors
    /// The driver's scan error for a spelling it cannot read as a time.
    pub fn client_sync_cursor(&self, remote: &str) -> Result<GoTime, String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        let mut statement = conn
            .prepare("SELECT last_sync FROM sync_state WHERE remote = ?")
            .map_err(|error| sql(&error))?;
        let mut rows = statement.query([remote]).map_err(|error| sql(&error))?;
        match rows.next().map_err(|error| sql(&error))? {
            None => Ok(GoTime::zero()),
            Some(row) => time_col(row, 0, "last_sync"),
        }
    }

    /// `SetSyncCursor`.
    ///
    /// # Errors
    /// The SQLite error text.
    pub fn client_set_sync_cursor(&self, remote: &str, at: &GoTime) -> Result<(), String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        conn.execute(
            "INSERT INTO sync_state (remote, last_sync) VALUES (?, ?)\n\t\t ON CONFLICT(remote) DO UPDATE SET last_sync = excluded.last_sync",
            params![remote, bound(at)],
        )
        .map(|_| ())
        .map_err(|error| sql(&error))
    }

    /// `GetMemoriesSinceCursorIDForSync` without vectors.
    ///
    /// # Errors
    /// The SQLite or scan error text.
    pub fn client_local_memories(
        &self,
        since: &GoTime,
        last_id: &str,
        limit: i64,
    ) -> Result<Vec<LocalMemory>, String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        let predicate = if last_id.is_empty() {
            "symbrain_sync_time(updated_at) > symbrain_sync_time(?)"
        } else {
            "(symbrain_sync_time(updated_at) > symbrain_sync_time(?) OR (symbrain_sync_time(updated_at) = symbrain_sync_time(?) AND id > ?))"
        };
        let query = format!(
            "SELECT {LITE}, embedding, embedding_source, embedding_model, embedding_quantization FROM memories WHERE ({predicate}) AND COALESCE(json_extract(metadata, '$.sync_exclude'), '') != 'true' ORDER BY symbrain_sync_time(updated_at) ASC, id ASC LIMIT ?"
        );
        let mut statement = conn.prepare(&query).map_err(|error| sql(&error))?;
        let since_text = bound(since);
        let mut rows = if last_id.is_empty() {
            statement.query(params![since_text, limit])
        } else {
            statement.query(params![since_text, since_text, last_id, limit])
        }
        .map_err(|error| sql(&error))?;
        let mut memories = Vec::new();
        while let Some(row) = rows.next().map_err(|error| sql(&error))? {
            memories.push(scan_lite(row)?);
        }
        drop(rows);
        drop(statement);
        let ids: Vec<String> = memories.iter().map(|memory| memory.id.clone()).collect();
        let mut extras = crate::sync_graph::extras_for(&conn, &ids).map_err(|error| sql(&error))?;
        for memory in &mut memories {
            memory.extras = extras.remove(&memory.id).unwrap_or_default();
        }
        Ok(memories)
    }

    /// `GetDeletedSinceCursorID`.
    ///
    /// # Errors
    /// The SQLite or scan error text.
    pub fn client_local_deletes(
        &self,
        since: &GoTime,
        last_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, GoTime)>, String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        let query = if last_id.is_empty() {
            "SELECT o.memory_id, o.ts FROM sync_oplog o\n\t\t\tWHERE o.op = 'delete'\n\t\t\t  AND symbrain_sync_time(o.ts) > symbrain_sync_time(?)\n\t\t\t  AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)\n\t\t\tORDER BY symbrain_sync_time(o.ts) ASC, o.memory_id ASC\n\t\t\tLIMIT ?"
        } else {
            "SELECT o.memory_id, o.ts FROM sync_oplog o\n\t\t\tWHERE o.op = 'delete'\n\t\t\t  AND (symbrain_sync_time(o.ts) > symbrain_sync_time(?) OR (symbrain_sync_time(o.ts) = symbrain_sync_time(?) AND o.memory_id > ?))\n\t\t\t  AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)\n\t\t\tORDER BY symbrain_sync_time(o.ts) ASC, o.memory_id ASC\n\t\t\tLIMIT ?"
        };
        let mut statement = conn.prepare(query).map_err(|error| sql(&error))?;
        let since_text = bound(since);
        let mut rows = if last_id.is_empty() {
            statement.query(params![since_text, limit])
        } else {
            statement.query(params![since_text, since_text, last_id, limit])
        }
        .map_err(|error| sql(&error))?;
        let mut deleted = Vec::new();
        while let Some(row) = rows.next().map_err(|error| sql(&error))? {
            deleted.push((text_col(row, 0, "memory_id")?, time_col(row, 1, "ts")?));
        }
        Ok(deleted)
    }

    /// `SyncUpsertMemoryIfNewer`: skipped when a local tombstone is not older
    /// than the incoming row, otherwise `UpsertMemoryIfNewer`.
    ///
    /// # Errors
    /// The SQLite, scan or encoding error text.
    pub fn client_upsert_if_newer(
        &self,
        memory: &ClientMemory,
        mode: ClientApplyMode,
    ) -> Result<bool, String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        if let Some((operation, at)) = latest_operation(&conn, &memory.id)?
            && operation == "delete"
            && !memory.updated_at.after(&at)
        {
            return Ok(false);
        }
        upsert(&conn, memory, mode)
    }

    /// `ApplyRemoteDelete`: `Ok(true)` when a local row was removed.
    ///
    /// # Errors
    /// The SQLite or scan error text.
    pub fn client_apply_remote_delete(
        &self,
        id: &str,
        deleted_at: &GoTime,
        audit_enabled: bool,
    ) -> Result<bool, String> {
        let conn = self.lock().map_err(|error| sql(&error))?;
        let transaction = conn.unchecked_transaction().map_err(|error| sql(&error))?;
        let local: Option<(GoTime, String, String, String)> = {
            let mut statement = transaction
                .prepare("SELECT updated_at, scope, created_by, created_session FROM memories WHERE id = ?")
                .map_err(|error| sql(&error))?;
            let mut rows = statement.query([id]).map_err(|error| sql(&error))?;
            match rows.next().map_err(|error| sql(&error))? {
                None => None,
                Some(row) => Some((
                    time_col(row, 0, "updated_at")?,
                    opt_text_col(row, 1)?,
                    opt_text_col(row, 2)?,
                    opt_text_col(row, 3)?,
                )),
            }
        };
        let present = local.is_some();
        if let Some((updated, ..)) = &local
            && updated.after(deleted_at)
        {
            return Ok(false);
        }
        if !present
            && let Some((operation, at)) = latest_operation(&transaction, id)?
            && operation == "delete"
            && !deleted_at.after(&at)
        {
            return Ok(false);
        }
        if present {
            transaction
                .execute("DELETE FROM memories WHERE id = ?", [id])
                .map_err(|error| sql(&error))?;
        }
        transaction
            .execute(
                "INSERT INTO sync_oplog (op, memory_id, ts) VALUES ('delete', ?, ?)",
                params![id, bound(deleted_at)],
            )
            .map_err(|error| sql(&error))?;
        transaction.commit().map_err(|error| sql(&error))?;
        if let Some((_, scope, created_by, created_session)) = local
            && audit_enabled
        {
            audit(
                &conn,
                "delete",
                id,
                &scope,
                &created_session,
                &created_by,
                "",
            );
        }
        Ok(present)
    }
}

/// `GetTombstone`'s read: the latest oplog operation of `id`.
fn latest_operation(conn: &Connection, id: &str) -> Result<Option<(String, GoTime)>, String> {
    let mut statement = conn
        .prepare("SELECT op, ts FROM sync_oplog WHERE memory_id = ? ORDER BY event_id DESC LIMIT 1")
        .map_err(|error| sql(&error))?;
    let mut rows = statement.query([id]).map_err(|error| sql(&error))?;
    match rows.next().map_err(|error| sql(&error))? {
        None => Ok(None),
        Some(row) => Ok(Some((text_col(row, 0, "op")?, time_col(row, 1, "ts")?))),
    }
}

/// `UpsertMemoryIfNewer`.
#[allow(clippy::too_many_lines)] // One linear pass mirroring the shipped upsert.
fn upsert(conn: &Connection, memory: &ClientMemory, mode: ClientApplyMode) -> Result<bool, String> {
    let vector = memory.embedding.clone().unwrap_or_default();
    let embedding = match &memory.embedding {
        None => "null".to_owned(),
        Some(vector) if vector.is_empty() => "[]".to_owned(),
        Some(vector) => embedding_json(vector),
    };
    let lsh = crate::lsh::compute_lsh(&vector)?;
    let binary = (mode.quantize_binary && !vector.is_empty()).then(|| sign_bits(&vector));
    let dimension = i64::try_from(vector.len()).unwrap_or(0);
    let metadata = metadata_json(memory.metadata.as_ref());
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
        .optional()
        .map_err(|error| sql(&error))?;
    let Some(existing) = existing else {
        conn.execute(
            "INSERT INTO memories (id, content, scope, metadata, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at)\n\t\t\t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                memory.id, memory.content, memory.scope, metadata, embedding, binary, dimension,
                memory.embedding_source, memory.embedding_model, memory.embedding_quantization,
                content_hash, lsh, bound(&memory.created_at), bound(&memory.updated_at),
                memory.created_by, memory.updated_by, memory.created_session, memory.updated_session,
                status, non_null(&memory.consolidated_into_id), memory.importance,
                optional_bound(memory.valid_from.as_ref()), optional_bound(memory.valid_to.as_ref()),
                non_null(&memory.superseded_by), memory.tier,
                optional_bound(memory.expires_at.as_ref()), memory.access_count,
                optional_bound(memory.last_access.as_ref()),
                optional_bound(memory.prev_access.as_ref()), review, memory.kind, decay,
                optional_bound(memory.retired_at.as_ref()),
            ],
        )
        .map_err(|error| sql(&error))?;
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
    let existing = time_value(ValueRef::from(&existing), 0, "updated_at")?;
    if !memory.updated_at.after(&existing) {
        if existing.after(&memory.updated_at) {
            return Ok(false);
        }
        return reconcile_equal(conn, memory, mode);
    }
    let columns = if vector.is_empty() {
        crate::sync_embedding::preserved(conn, &memory.id, &memory.content)
            .map_err(|error| sql(&error))?
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
            content_hash, columns.lsh, bound(&memory.updated_at), memory.updated_by,
            memory.updated_session, status, non_null(&memory.consolidated_into_id),
            optional_bound(memory.valid_from.as_ref()), optional_bound(memory.valid_to.as_ref()),
            non_null(&memory.superseded_by), memory.importance, memory.tier, optional_bound(memory.expires_at.as_ref()),
            memory.access_count, optional_bound(memory.last_access.as_ref()),
            optional_bound(memory.prev_access.as_ref()), review, memory.kind, decay,
            optional_bound(memory.retired_at.as_ref()), memory.id,
        ],
    )
    .map_err(|error| sql(&error))?;
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
    memory: &ClientMemory,
    mode: ClientApplyMode,
) -> Result<bool, String> {
    let incoming = crate::sync_lifecycle::Lifecycle {
        retired_at: optional_bound(memory.retired_at.as_ref()),
        superseded_by: memory.superseded_by.clone(),
        consolidated_into_id: memory.consolidated_into_id.clone(),
        valid_from: optional_bound(memory.valid_from.as_ref()),
        valid_to: optional_bound(memory.valid_to.as_ref()),
    };
    let audited = crate::sync_lifecycle::Audited {
        enabled: mode.audit_enabled,
        scope: &memory.scope,
        session: &memory.updated_session,
        actor: &memory.updated_by,
    };
    crate::sync_lifecycle::reconcile_equal(conn, &memory.id, &incoming, &audited)
        .map_err(|error| sql(&error))
}
