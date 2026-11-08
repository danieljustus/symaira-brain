//! SQL and logging helpers of the consolidation engine (split from
//! `consolidation.rs`; see there for the port notes).

use crate::consolidation::ConsolidatedMemory;

/// A memory to insert together with its embedding.
pub(crate) struct NewRow {
    pub memory: ConsolidatedMemory,
    pub vector: Vec<f32>,
    pub source: String,
    pub model: String,
    pub now: DateTime<Utc>,
}
use crate::model::StoreError;
use crate::{embedding, gotime, lsh, write};
use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};

pub(crate) fn update_status(
    tx: &Transaction<'_>,
    id: &str,
    status: &str,
    parent: Option<&str>,
    now: DateTime<Utc>,
) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE memories SET consolidation_status = ?, consolidated_into_id = ?, updated_at = ? WHERE id = ?",
        params![status, parent, gotime::format(now), id],
    )
}

pub(crate) fn save_new(tx: &Transaction<'_>, row: &NewRow) -> Result<(), String> {
    let NewRow {
        memory,
        vector,
        source,
        model,
        now,
    } = row;
    let metadata = serde_json::to_string(&memory.metadata).map_err(|e| e.to_string())?;
    let lsh = lsh::compute_lsh(vector)?;
    let stamp = gotime::format(*now);
    tx.execute(
        "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) \
         VALUES(?,?,?,?,?,NULL,?,?,?,'',?,?,?,?,'','','','',?,NULL,0,?,NULL,NULL,'long_term',NULL,1,NULL,NULL,'approved','',1.0,NULL)",
        params![
            memory.id,
            memory.content,
            memory.scope,
            metadata,
            write::embedding_text(vector),
            i64::try_from(vector.len()).unwrap_or(0),
            source,
            model,
            write::content_hash(&memory.content),
            lsh,
            stamp,
            stamp,
            memory.consolidation_status,
            stamp
        ],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

pub(crate) fn report_failures(
    failures: &[(String, String)],
    succeeded: usize,
) -> Result<(), StoreError> {
    for (scope, error) in failures {
        eprintln!("consolidation: skipping scope {scope:?} after LLM failure: {error}");
    }
    eprintln!(
        "consolidation: run summary: {succeeded} scope(s) succeeded, {} scope(s) skipped due to LLM failures",
        failures.len()
    );
    if succeeded == 0 && !failures.is_empty() {
        return Err(StoreError::Invalid(format!(
            "consolidation failed: all scopes failed ({}): first error for scope {:?}: {}",
            failures.len(),
            failures[0].0,
            failures[0].1
        )));
    }
    Ok(())
}

/// Default embedder: hash fallback in the shipped embedding space.
#[must_use]
pub fn hash_embed(text: &str) -> (Vec<f32>, String, String) {
    (
        embedding::local_hash_vector(text),
        embedding::HASH_FALLBACK_SOURCE.to_owned(),
        String::new(),
    )
}
