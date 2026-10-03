//! Frozen CLI direct-write persistence, separate from the MCP migration surface.

use crate::{EmbeddingGenerator, Store, StoreError};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;

/// A validated direct CLI write. The caller must retain Go fallback for PII,
/// secondary extraction and enabled conflict checking.
pub struct DirectWrite {
    pub content: String,
    pub scope: String,
    pub kind: String,
    pub metadata: BTreeMap<String, String>,
    pub author: String,
    pub entities: Vec<String>,
    pub staged: bool,
    pub quantize_binary: bool,
    pub conflict_enabled: bool,
}

impl Store {
    /// Persists the extraction-free, conflict-free CLI service contract.
    ///
    /// # Errors
    /// Returns a SQLite or embedding representation error.
    pub fn set_direct_cli(
        &self,
        request: &DirectWrite,
        generator: &EmbeddingGenerator,
    ) -> Result<String, StoreError> {
        if !crate::direct_content_supported(&request.content)
            || request
                .metadata
                .values()
                .any(|value| !crate::direct_text_supported(value))
            || (!request.staged && request.conflict_enabled)
            || !["", "global", "project", "agent", "user", "session"]
                .contains(&request.scope.as_str())
            || !["user", "feedback", "project", "reference"].contains(&request.kind.as_str())
        {
            return Err(StoreError::Invalid(
                "direct CLI write requires the delegated Go pipeline".into(),
            ));
        }
        let observed_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let mut metadata: BTreeMap<String, String> = [
            ("source_type", "direct"),
            ("source_tool", "symbrain-cli"),
            ("source_uri", ""),
            ("authority", "direct"),
            ("confidence", "high"),
            ("verification_status", "unverified"),
            ("sensitivity", "internal"),
            ("sharing_level", "private"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        metadata.insert("observed_at".into(), observed_at);
        metadata.extend(request.metadata.clone());
        let scope = if request.scope.is_empty() {
            "global"
        } else {
            &request.scope
        };
        if scope == "project" {
            let name = active_project();
            if !crate::direct_text_supported(&name) {
                return Err(StoreError::Invalid(
                    "project provenance requires the delegated Go pipeline".into(),
                ));
            }
            metadata.insert("project_name".into(), name);
        }
        let valid_from = timestamp();
        let embedding = generator.generate(&request.content);
        let model = if embedding.source == "ollama" {
            generator.model()
        } else {
            ""
        };
        let binary = request
            .quantize_binary
            .then(|| sign_bits(&embedding.vector));
        let metadata = go_json(&metadata)?;
        let id = crate::write::new_uuid();
        let created_at = timestamp();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'','','raw',NULL,0,?,NULL,NULL,'long_term',NULL,1,NULL,NULL,'approved','',1,NULL)",
            params![id, request.content, scope, metadata, embedding_json(&embedding.vector), binary,
                i64::try_from(embedding.vector.len()).unwrap_or(768), embedding.source, model, "",
                crate::write::content_hash(&request.content), crate::lsh::compute_lsh(&embedding.vector).map_err(StoreError::Invalid)?,
                created_at, created_at, request.author, request.author, valid_from],
        ).map_err(|error| StoreError::Invalid(format!("failed to save memory: {error}")))?;
        audit(&conn, "set", &id, scope, "", &request.author, "");
        crate::cli_entity::link(&conn, &id, &request.entities, &request.author);
        conn.execute(
            "UPDATE memories SET kind=?,updated_at=? WHERE id=?",
            params![request.kind, timestamp(), id],
        )?;
        if request.staged {
            conn.execute(
                "UPDATE memories SET review_status='staged',updated_at=? WHERE id=?",
                params![timestamp(), id],
            )?;
        }
        Ok(id)
    }

    /// Deletes through the CLI service's access-feedback and audit sequence.
    ///
    /// # Errors
    /// Returns a SQLite error; audit errors do not fail a completed deletion.
    pub fn delete_cli(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        if !crate::cli_delete_admission::checked(&conn, id)? {
            return Ok(false);
        }
        conn.execute("UPDATE memories SET access_count=access_count+1,prev_access=last_access,last_access=? WHERE id=?",
            params![timestamp(), id])?;
        // Go reads the audit identity in DeleteMemory after GetMemory's access
        // update. If another process already deleted it, DeleteMemory succeeds.
        let identity: Option<(String, String, String)> = conn.query_row(
            "SELECT COALESCE(scope,''),COALESCE(created_by,''),COALESCE(created_session,'') FROM memories WHERE id=?", [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((scope, author, session)) = identity else {
            return Ok(true);
        };
        conn.execute("DELETE FROM memories WHERE id=?", [id])?;
        audit(&conn, "delete", id, &scope, &session, &author, "");
        Ok(true)
    }
}

pub(crate) fn audit(
    conn: &Connection,
    action: &str,
    id: &str,
    scope: &str,
    session: &str,
    author: &str,
    detail: &str,
) {
    let nonempty = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    let _ = conn.execute("INSERT INTO audit_log(id,action,memory_id,scope,session,actor,detail,created_at) VALUES(?,?,?,?,?,?,?,?)",
        params![crate::write::new_uuid(), action, nonempty(id), nonempty(scope), nonempty(session), nonempty(author), nonempty(detail), timestamp()]);
}

pub(crate) fn go_json(value: &impl serde::Serialize) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)
        .map_err(|error| StoreError::Invalid(error.to_string()))?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn active_project() -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return "default_project".into();
    };
    let selected = cwd
        .ancestors()
        .find(|path| path.join(".symmemory.toml").exists() || path.join(".git").exists())
        .unwrap_or(&cwd);
    selected.file_name().map_or_else(
        || std::path::MAIN_SEPARATOR.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

// encoding/json uses the float32 shortest representation with its own fixed/
// scientific boundary. Keep negative zero and normalize exponent signs only.
fn embedding_json(vector: &[f32]) -> String {
    let values = vector
        .iter()
        .map(|value| {
            let magnitude = value.abs();
            if magnitude != 0.0 && !(1e-6..1e21).contains(&magnitude) {
                let scientific = format!("{value:e}");
                let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
                let exponent = exponent.parse::<i32>().unwrap_or(0);
                format!("{mantissa}e{exponent:+}")
            } else {
                format!("{value}")
            }
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn sign_bits(vector: &[f32]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 96];
    for (index, value) in vector.iter().take(768).enumerate() {
        if *value <= 0.0 {
            bytes[index / 8] |= 1 << (index % 8);
        }
    }
    bytes
}

/// Reports whether project provenance bypasses the frozen PII pipeline.
#[must_use]
pub fn direct_project_supported() -> bool {
    crate::direct_text_supported(&active_project())
}

fn timestamp() -> String {
    let now = Utc::now();
    let base = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = now.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base} +0000 UTC")
    } else {
        let fraction = format!("{nanos:09}");
        format!("{base}.{} +0000 UTC", fraction.trim_end_matches('0'))
    }
}
