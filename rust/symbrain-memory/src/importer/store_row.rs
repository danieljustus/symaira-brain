//! The raw `agent`-scope memory row the importers write (shared by the
//! registry and the memory-tool smart importer).

use crate::model::{Store, StoreError};
use crate::{gotime, lsh, write};
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::collections::BTreeMap;

/// Everything one imported memory row needs.
pub(super) struct AgentRow<'a> {
    pub id: &'a str,
    pub content: &'a str,
    /// Stored as given (the smart importer hashes the unredacted text).
    pub content_hash: &'a str,
    pub metadata: &'a BTreeMap<String, String>,
    pub created: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub staged: bool,
    /// Vector, embedding source and model.
    pub embedding: (&'a [f32], &'a str, &'a str),
}

/// Inserts the row (`SaveMemory`) and records the `set` audit event.
pub(super) fn insert_agent_memory(store: &Store, row: &AgentRow<'_>) -> Result<(), StoreError> {
    let wrap = |e: String| StoreError::Invalid(format!("failed to save memory: {e}"));
    let (vector, source, model) = row.embedding;
    let metadata = serde_json::to_string(row.metadata).map_err(|e| wrap(e.to_string()))?;
    let lsh = lsh::compute_lsh(vector).map_err(wrap)?;
    let (created, now) = (gotime::format(row.created), gotime::format(row.now));
    let conn = store.lock()?;
    conn.execute(
        "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) \
         VALUES(?,?,'agent',?,?,NULL,?,?,?,'',?,?,?,?,'','','','','raw',NULL,0,?,NULL,NULL,'long_term',NULL,1,NULL,NULL,?,'',1.0,NULL)",
        params![
            row.id,
            row.content,
            metadata,
            write::embedding_text(vector),
            i64::try_from(vector.len()).unwrap_or(0),
            source,
            model,
            row.content_hash,
            lsh,
            created,
            now,
            now,
            if row.staged { "staged" } else { "approved" }
        ],
    )
    .map_err(|e| wrap(e.to_string()))?;
    crate::cli_write::audit(&conn, "set", row.id, "agent", "", "", "");
    Ok(())
}
