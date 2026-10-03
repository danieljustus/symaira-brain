//! Native memory writes, identifiers and embedding persistence.

use crate::model::{Memory, SetOptions, Store, StoreError};
use crate::store::{redact_text, redact_value};
use chrono::Utc;
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
const MAX_MEMORY_CONTENT: usize = 64 * 1024;
const MAX_MEMORY_LABEL: usize = 256;

impl Store {
    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn set(
        &self,
        content: &str,
        scope: &str,
        kind: &str,
        metadata: serde_json::Map<String, Value>,
        staged: bool,
    ) -> Result<Memory, StoreError> {
        self.set_with_options(
            content,
            scope,
            kind,
            metadata,
            &SetOptions {
                staged,
                ..SetOptions::default()
            },
        )
    }

    /// Saves a memory and its optional session/entity provenance.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, redaction, or SQLite access fails.
    pub fn set_with_options(
        &self,
        content: &str,
        scope: &str,
        kind: &str,
        metadata: serde_json::Map<String, Value>,
        options: &SetOptions,
    ) -> Result<Memory, StoreError> {
        validate_set(content, scope, kind)?;
        let content = redact_text(content);
        let mut metadata_value = Value::Object(metadata);
        redact_value(&mut metadata_value);
        let metadata = metadata_value.as_object().cloned().unwrap_or_default();
        let now = Utc::now();
        let session_id = redact_text(&options.session_id);
        let entities = options
            .entities
            .iter()
            .map(|name| redact_text(name.trim()))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        let id = new_uuid();
        let review = if options.staged { "staged" } else { "approved" };
        let metadata_text = serde_json::to_string(&metadata)
            .map_err(|error| StoreError::Invalid(format!("metadata: {error}")))?;
        let now_text = crate::gotime::format(now);
        let tier = if options.working {
            "working"
        } else {
            "long_term"
        };
        let expires_at = options
            .working
            .then(|| crate::gotime::format(now + chrono::Duration::hours(24)));
        let actor = if options.actor.is_empty() {
            "mcp".to_owned()
        } else {
            options.actor.clone()
        };
        // The shipped store derives the vector at write time and stores both
        // the vector and its LSH bucket, so a memory written here is findable
        // by the shipped retrieval path and vice versa.
        let vector = crate::embedding::local_hash_vector(&content);
        let lsh = crate::lsh::compute_lsh(&vector)
            .map_err(|error| StoreError::Invalid(format!("memory set: {error}")))?;
        let conn = self.lock()?;
        // Column set and deterministic values mirror the shipped insert, so a
        // row written here is indistinguishable from a shipped one.
        conn.execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) \
             VALUES(?,?,?,?,?,NULL,?,?,?,?,?,?,?,?,?,?,?,?,?,NULL,?,?,NULL,NULL,?,?,?,NULL,NULL,?,?,?,NULL)",
            params![
                id,
                content,
                scope,
                metadata_text,
                embedding_text(&vector),
                i64::try_from(crate::embedding::DIMENSIONS).unwrap_or(768),
                "hash-fallback",
                "",
                "",
                content_hash(&content),
                lsh,
                now_text,
                now_text,
                actor,
                actor,
                session_id,
                session_id,
                "raw",
                0.0_f64,
                now_text,
                tier,
                expires_at,
                1_i64,
                review,
                kind,
                1.0_f64
            ],
        )?;
        for entity in &entities {
            let entity_id = stable_id(&["entity", &entity.to_lowercase()]);
            conn.execute(
                "INSERT OR IGNORE INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
                params![entity_id, entity, "other", "[]", "", "mcp", now_text, now_text],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
                params![id, entity_id],
            )?;
        }
        Ok(Memory {
            id,
            content,
            scope: scope.into(),
            metadata,
            created_at: now,
            updated_at: now,
            created_by: "mcp".into(),
            created_session: session_id.clone(),
            entities,
            consolidation_status: "raw".into(),
            kind: kind.into(),
            importance: 0.5,
        })
    }
}

fn validate_set(content: &str, scope: &str, kind: &str) -> Result<(), StoreError> {
    if content.trim().is_empty() {
        return Err(StoreError::Invalid(
            "invalid arguments for 'memory_set': 'content' is required".into(),
        ));
    }
    if content.chars().count() > MAX_MEMORY_CONTENT {
        return Err(StoreError::Invalid(format!(
            "invalid arguments for 'memory_set': content exceeds {MAX_MEMORY_CONTENT} characters"
        )));
    }
    if scope.chars().count() > MAX_MEMORY_LABEL || kind.chars().count() > MAX_MEMORY_LABEL {
        return Err(StoreError::Invalid(format!(
            "invalid arguments for 'memory_set': scope/kind exceeds {MAX_MEMORY_LABEL} characters"
        )));
    }
    if kind.trim().is_empty() {
        return Err(StoreError::Invalid(
            "invalid arguments for 'memory_set': 'kind' is required".into(),
        ));
    }
    Ok(())
}

/// Generates a UUID v4 string, the identifier shape the shipped store uses.
fn new_uuid() -> String {
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Fall back to the deterministic form rather than failing a write.
        return stable_id(&["memory", "fallback"]);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// SHA-256 hex digest of the content, as the shipped store records it.
fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut hash = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// The embedding column as the shipped store writes it.
///
/// The shipped encoder renders a `float32` with its shortest round-trip form
/// and without a forced fraction (`0`, not `0.0`), which `serde_json` would
/// not produce, so the array is rendered by hand.
fn embedding_text(vector: &[f32]) -> String {
    let values = vector
        .iter()
        .map(|value| format!("{value}"))
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn stable_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let hex = format!("{:x}", hasher.finalize());
    format!("memory-{}", &hex[..32])
}
