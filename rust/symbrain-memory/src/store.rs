use std::fs;
use std::path::Path;

use chrono::Utc;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::activity::{ActivityPage, ActivitySearch};
use crate::model::{Memory, Store, StoreError};
use crate::rows::{row_memory, row_memory_score};
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
        let store = Self::configure(Connection::open(path)?)?;
        #[cfg(unix)]
        crate::migration::secure_files(path)?;
        Ok(store)
    }

    /// Executes a bounded store operation.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation, SQLite, or filesystem access fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::configure(Connection::open_in_memory()?)
    }

    fn configure(conn: Connection) -> Result<Self, StoreError> {
        crate::migration::configure(conn)
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
