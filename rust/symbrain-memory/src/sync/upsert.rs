//! Frozen storage-level LWW ingestion; not the governed CLI set pipeline.

use super::store::parse_time;
use super::{SyncMemory, SyncTime};
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params};

impl Store {
    /// Applies a peer row only when strictly newer than a row/delete tombstone.
    /// Remote IDs, clocks, metadata and authors are preserved. Linked entities
    /// and evidence in the payload are not persisted by the Go sync upsert.
    /// # Errors
    /// Returns a database, timestamp, embedding or JSON encoding error.
    pub fn upsert_sync_memory(
        &self,
        memory: &SyncMemory,
        quantize_binary: bool,
    ) -> Result<bool, StoreError> {
        // Go checks the tombstone before serializing/calculating the upsert.
        let conn = self.lock()?;
        let tombstone: Option<(String, String)> = conn
            .query_row(
                "SELECT op,ts FROM sync_oplog WHERE memory_id=? ORDER BY event_id DESC LIMIT 1",
                [&memory.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((op, stamp)) = tombstone {
            if op == "delete" && memory.updated_at <= parse_time(&stamp)? {
                return Ok(false);
            }
        }
        let metadata = crate::cli_write::go_json(&memory.metadata)?;
        let embedding = crate::cli_write::go_json(&memory.embedding)?;
        let vector = memory.embedding.as_deref().unwrap_or_default();
        let lsh = crate::lsh::compute_lsh(vector).map_err(StoreError::Invalid)?;
        let dim =
            i64::try_from(vector.len()).map_err(|error| StoreError::Invalid(error.to_string()))?;
        let binary = (quantize_binary && !vector.is_empty()).then(|| sign_bits(vector));
        let hash = if memory.content_hash.is_empty() {
            crate::write::content_hash(&memory.content)
        } else {
            memory.content_hash.clone()
        };
        let status = if memory.consolidation_status.is_empty() {
            "raw"
        } else {
            &memory.consolidation_status
        };
        let review = if memory.review_status.is_empty() {
            "approved"
        } else {
            &memory.review_status
        };
        let decay = if memory.decay_factor <= 0.0 || memory.decay_factor > 1.0 {
            1.0
        } else {
            memory.decay_factor
        };
        let existing: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM memories WHERE id=?",
                [&memory.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(stamp) = &existing {
            if memory.updated_at <= parse_time(stamp)? {
                return Ok(false);
            }
        }
        if existing.is_none() {
            conn.execute("INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![memory.id,memory.content,memory.scope,metadata,embedding,binary,dim,
                    memory.embedding_source,memory.embedding_model,memory.embedding_quantization,hash,lsh,
                    memory.created_at.sqlite(),memory.updated_at.sqlite(),memory.created_by,memory.updated_by,
                    memory.created_session,memory.updated_session,status,nonempty(&memory.consolidated_into_id),
                    memory.importance,optional(memory.valid_from),optional(memory.valid_to),nonempty(&memory.superseded_by),
                    memory.tier,optional(memory.expires_at),memory.access_count,optional(memory.last_access),
                    optional(memory.prev_access),review,memory.kind,decay,optional(memory.retired_at)])?;
            crate::cli_write::audit(
                &conn,
                "set",
                &memory.id,
                &memory.scope,
                &memory.created_session,
                &memory.created_by,
                "",
            );
        } else {
            // Go deliberately does not update creation identity, validity or
            // superseded_by; no transactional batch is introduced here.
            conn.execute("UPDATE memories SET content=?,scope=?,metadata=?,embedding=?,embedding_binary=?,embedding_dim=?,embedding_source=?,embedding_model=?,embedding_quantization=?,content_hash=?,lsh_hash=?,updated_at=?,updated_by=?,updated_session=?,consolidation_status=?,consolidated_into_id=?,importance=?,tier=?,expires_at=?,access_count=?,last_access=?,prev_access=?,review_status=?,kind=?,decay_factor=?,retired_at=? WHERE id=?",
                params![memory.content,memory.scope,metadata,embedding,binary,dim,
                    memory.embedding_source,memory.embedding_model,memory.embedding_quantization,hash,lsh,
                    memory.updated_at.sqlite(),memory.updated_by,memory.updated_session,status,
                    nonempty(&memory.consolidated_into_id),memory.importance,memory.tier,optional(memory.expires_at),
                    memory.access_count,optional(memory.last_access),optional(memory.prev_access),review,memory.kind,decay,
                    optional(memory.retired_at),memory.id])?;
            crate::cli_write::audit(
                &conn,
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
}

fn optional(time: Option<SyncTime>) -> Option<String> {
    time.map(SyncTime::sqlite)
}
fn nonempty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}
fn sign_bits(vector: &[f32]) -> Vec<u8> {
    let mut bits = vec![0_u8; 96];
    for (index, value) in vector.iter().enumerate() {
        if *value <= 0.0 {
            bits[index / 8] |= 1 << (index % 8);
        }
    }
    bits
}
