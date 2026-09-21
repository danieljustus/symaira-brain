use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::activity::{ActivityPage, ActivitySearch};
use crate::model::{Memory, SetOptions, Store, StoreError};
use crate::rows::{row_memory, row_memory_score};
use crate::schema::{INDEXES, MIGRATIONS, SCHEMA};
const MAX_MEMORY_CONTENT: usize = 64 * 1024;
const MAX_MEMORY_LABEL: usize = 256;
const MAX_LIST_RESULTS: usize = 1000;

impl Store {
    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| StoreError::Io(format!("create memory directory: {error}")))?;
        }
        Self::configure(Connection::open(path)?)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::configure(Connection::open_in_memory()?)
    }

    fn configure(conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        apply_column_parity(&conn)?;
        // The indexes cover columns an older database only gains through the
        // parity repair above, so they are created after it: applied together
        // with the tables they would fail on a legacy `memories` that has no
        // `tier` yet.
        conn.execute_batch(INDEXES)?;
        let transaction = conn.unchecked_transaction()?;
        for version in MIGRATIONS {
            transaction.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?)",
                [version],
            )?;
        }
        transaction.commit()?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.conn
            .lock()
            .map_err(|_| StoreError::Invalid("memory store lock poisoned".into()))
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn get(&self, id: &str) -> Result<Option<Memory>, StoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE id=?",
            [id],
            row_memory,
        )
        .optional()
        .map_err(Into::into)
    }

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

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    /// Reads `memory list` rows in the shipped lite projection.
    ///
    /// # Errors
    /// Returns a `StoreError` when the scan fails.
    pub fn list_lite(
        &self,
        scope: &str,
        limit: usize,
    ) -> Result<Vec<crate::list_rows::MemoryListRow>, StoreError> {
        // The shipped scan treats a non-positive limit as its default of 1000
        // rows, which a `memory list` invocation without `--limit` relies on.
        let limit = if limit == 0 {
            1000
        } else {
            limit.min(MAX_LIST_RESULTS)
        };
        let limit = limit_i64(limit);
        let conn = self.lock()?;
        crate::list_rows::list_lite(&conn, scope, limit).map_err(Into::into)
    }

    /// Reads `memory rules` rows in the shipped shape.
    ///
    /// # Errors
    /// Returns a `StoreError` when the scan fails.
    pub fn list_rules(&self, scope: &str) -> Result<Vec<crate::list_rows::RuleRow>, StoreError> {
        let conn = self.lock()?;
        crate::list_rows::list_rules(&conn, scope).map_err(Into::into)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn list(&self, scope: &str, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let limit = limit_i64(limit.min(MAX_LIST_RESULTS));
        let now = crate::gotime::format(Utc::now());
        let conn = self.lock()?;
        let sql = if scope.is_empty() {
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE review_status != 'staged' AND (expires_at IS NULL OR expires_at > ?) ORDER BY created_at DESC,id DESC LIMIT ?"
        } else {
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE scope=? AND review_status != 'staged' AND (expires_at IS NULL OR expires_at > ?) ORDER BY created_at DESC,id DESC LIMIT ?"
        };
        let mut statement = conn.prepare(sql)?;
        let rows = if scope.is_empty() {
            statement.query_map(params![now, limit], row_memory)?
        } else {
            statement.query_map(params![scope, now, limit], row_memory)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn search(
        &self,
        query: &str,
        scope: &str,
        limit: usize,
    ) -> Result<Vec<(Memory, f32)>, StoreError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let now = crate::gotime::format(Utc::now());
        let conn = self.lock()?;
        let sql = if scope.is_empty() {
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE review_status != 'staged' AND (expires_at IS NULL OR expires_at > ?) AND lower(content) LIKE ? ORDER BY updated_at DESC,id DESC LIMIT ?"
        } else {
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE review_status != 'staged' AND scope=? AND (expires_at IS NULL OR expires_at > ?) AND lower(content) LIKE ? ORDER BY updated_at DESC,id DESC LIMIT ?"
        };
        let mut statement = conn.prepare(sql)?;
        let rows = if scope.is_empty() {
            statement.query_map(
                params![now, pattern, limit_i64(limit.min(MAX_LIST_RESULTS))],
                row_memory_score,
            )?
        } else {
            statement.query_map(
                params![scope, now, pattern, limit_i64(limit.min(MAX_LIST_RESULTS))],
                row_memory_score,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn candidates(&self, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let conn = self.lock()?;
        let now = crate::gotime::format(Utc::now());
        let mut statement = conn.prepare(
            "SELECT id,content,scope,metadata,created_at,updated_at,created_by,created_session,consolidation_status,kind,importance,COALESCE((SELECT json_group_array(e.name) FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=memories.id),'[]') FROM memories WHERE review_status='staged' AND (expires_at IS NULL OR expires_at > ?) ORDER BY created_at ASC,id ASC LIMIT ?",
        )?;
        statement
            .query_map(
                params![now, limit_i64(limit.min(MAX_LIST_RESULTS))],
                row_memory,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn promote(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE memories SET review_status='approved',updated_at=? WHERE id=? AND review_status='staged'",
            params![crate::gotime::format(Utc::now()), id],
        )? == 1)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn reject(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "DELETE FROM memories WHERE id=? AND review_status='staged'",
            [id],
        )? == 1)
    }

    /// Deletes a memory by ID.
    ///
    /// # Errors
    /// Returns a `StoreError` when SQLite access fails.
    pub fn delete(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let _ = conn.execute("DELETE FROM memory_entities WHERE memory_id=?", [id]);
        Ok(conn.execute("DELETE FROM memories WHERE id=?", [id])? > 0)
    }

    /// # Errors
    /// Returns a `StoreError` when validation or the SQLite query fails.
    pub fn activity_search(&self, options: &ActivitySearch) -> Result<ActivityPage, StoreError> {
        crate::activity::search(self, options)
    }

    /// Runs the shipped retrieval pipeline for a prepared query vector.
    ///
    /// `query_source` names the embedding space of `query_vector`; the shipped
    /// path only scores rows from the same space.
    ///
    /// # Errors
    /// Returns a `StoreError` when the vector does not fit the embedding
    /// dimension or SQLite access fails.
    pub fn search_ranked(
        &self,
        query_vector: &[f32],
        query_source: &str,
        scope: &str,
        limit: usize,
    ) -> Result<Vec<crate::search_rows::SearchHit>, StoreError> {
        let conn = self.lock()?;
        crate::retrieval::search(&conn, query_vector, query_source, scope, limit)
    }

    pub(crate) fn activity_conn(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.lock()
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

pub(crate) fn redact_text(text: &str) -> String {
    static URL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static ASSIGNMENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let url = URL.get_or_init(|| {
        Regex::new(r"(?i)([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^@\s]+@").expect("static redaction regex")
    });
    let assignment = ASSIGNMENT.get_or_init(|| Regex::new(r"(?i)(\b(?:api[_-]?key|auth[_-]?token|access[_-]?token|client[_-]?secret|private[_-]?key|password|passwd|secret|token)\b\s*[:=]\s*)[A-Za-z0-9_.-]{20,}").expect("static redaction regex"));
    let clean = url.replace_all(text, "$1[REDACTED]@");
    assignment.replace_all(&clean, "$1[REDACTED]").into_owned()
}

pub(crate) fn redact_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = redact_text(text),
        Value::Array(values) => values.iter_mut().for_each(redact_value),
        Value::Object(values) => values.values_mut().for_each(redact_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn limit_i64(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// Adds any shipped column an existing native database is missing.
fn apply_column_parity(conn: &Connection) -> Result<(), StoreError> {
    for (table, column, definition) in crate::schema::COLUMN_PARITY {
        let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let present = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == column);
        drop(statement);
        if !present {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn sync_oplog_tracks_memory_lifecycle_and_excludes_marked_rows() {
        let store = Store::open_in_memory().expect("open store");
        let memory = store
            .set("fact", "global", "note", serde_json::Map::new(), false)
            .expect("set memory");

        let entries = |store: &Store| {
            let conn = store.lock().expect("lock store");
            let mut statement = conn
                .prepare("SELECT op, memory_id FROM sync_oplog ORDER BY event_id")
                .expect("prepare oplog query");
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .expect("query oplog")
                .collect::<Result<Vec<_>, _>>()
                .expect("read oplog")
        };

        assert_eq!(entries(&store), vec![("upsert".into(), memory.id.clone())]);
        assert!(store.delete(&memory.id).expect("delete memory"));
        assert_eq!(
            entries(&store),
            vec![
                ("upsert".into(), memory.id.clone()),
                ("delete".into(), memory.id),
            ]
        );

        let mut metadata = serde_json::Map::new();
        metadata.insert("sync_exclude".into(), Value::String("true".into()));
        store
            .set("local activity", "global", "note", metadata, false)
            .expect("set excluded memory");
        assert_eq!(entries(&store).len(), 2);
    }
}
