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
use crate::schema::{MIGRATIONS, SCHEMA};
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
        let id = stable_id(&["memory", &content, scope, &now.to_rfc3339()]);
        let review = if options.staged { "staged" } else { "approved" };
        let metadata_text = serde_json::to_string(&metadata)
            .map_err(|error| StoreError::Invalid(format!("metadata: {error}")))?;
        let now_text = now.to_rfc3339();
        let tier = if options.working {
            "working"
        } else {
            "long_term"
        };
        let expires_at = options
            .working
            .then(|| (now + chrono::Duration::hours(24)).to_rfc3339());
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at,created_by,updated_by,created_session,updated_session,review_status,kind,tier,expires_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![id, content, scope, metadata_text, "", now_text, now_text, "mcp", "mcp", session_id, session_id, review, kind, tier, expires_at],
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
    pub fn list(&self, scope: &str, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let limit = limit_i64(limit.min(MAX_LIST_RESULTS));
        let now = Utc::now().to_rfc3339();
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
        let now = Utc::now().to_rfc3339();
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
        let now = Utc::now().to_rfc3339();
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
            params![Utc::now().to_rfc3339(), id],
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

    /// # Errors
    /// Returns a `StoreError` when validation or the SQLite query fails.
    pub fn activity_search(&self, options: &ActivitySearch) -> Result<ActivityPage, StoreError> {
        crate::activity::search(self, options)
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

fn redact_text(text: &str) -> String {
    static URL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static ASSIGNMENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let url = URL.get_or_init(|| {
        Regex::new(r"(?i)([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^@\s]+@").expect("static redaction regex")
    });
    let assignment = ASSIGNMENT.get_or_init(|| Regex::new(r"(?i)(\b(?:api[_-]?key|auth[_-]?token|access[_-]?token|client[_-]?secret|private[_-]?key|password|passwd|secret|token)\b\s*[:=]\s*)[A-Za-z0-9_.-]{20,}").expect("static redaction regex"));
    let clean = url.replace_all(text, "$1[REDACTED]@");
    assignment.replace_all(&clean, "$1[REDACTED]").into_owned()
}

fn redact_value(value: &mut Value) {
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

fn stable_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let hex = format!("{:x}", hasher.finalize());
    format!("memory-{}", &hex[..32])
}
