//! `symbrain memory set` against any store state: the whole shipped write
//! pipeline (`memory.Store` + `MemoryService.SetGoverned`) over raw bytes.
//!
//! Prepare (scope check, PII redaction, provenance/trust/policy metadata),
//! embedding, the optional conflict check, entity linking, secondary-fact
//! extraction with evidence, the redaction audit event, and finally the
//! governance steps (kind, staged review state).

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, ToSql, params, types::ToSqlOutput, types::ValueRef};
use sha2::{Digest, Sha256};

use crate::conflict::{self, ConflictConfig, NewMemory, Resolution, Verdict};
use crate::evidence::SourceRef;
use crate::extract;
use crate::gotext::json_string;
use crate::search_rows::SearchRow;
use crate::slog::{LogSink, quote_bytes};
use crate::{EmbeddingGenerator, Store, StoreError};

const SOURCE_TOOL: &str = "symbrain-cli";

/// A validated CLI write, in the byte-exact form the flags arrived in.
pub struct CliSet {
    /// Trimmed, non-empty content.
    pub content: Vec<u8>,
    /// The `--scope` flag value.
    pub scope: Vec<u8>,
    /// Canonical kind.
    pub kind: String,
    /// Decoded `--metadata` object.
    pub metadata: BTreeMap<String, String>,
    /// The `--author` flag value.
    pub author: Vec<u8>,
    /// Trimmed, non-empty entity names.
    pub entities: Vec<Vec<u8>>,
    /// `--staged`.
    pub staged: bool,
    /// `hybrid_search.quantize_to_binary`.
    pub quantize_binary: bool,
    /// `hybrid_search.prefilter_enabled`.
    pub prefilter: bool,
    /// The write-path conflict configuration.
    pub conflict: ConflictConfig,
}

/// How the shipped SQLite driver renders a statement failure:
/// `<primary code text>: <message> (<extended code>)`.
pub(crate) fn go_sqlite_error(error: &rusqlite::Error) -> String {
    match error {
        rusqlite::Error::SqliteFailure(failure, message) => format!(
            "{}: {} ({})",
            rusqlite::ffi::code_to_str(failure.extended_code & 0xff),
            message.clone().unwrap_or_default(),
            failure.extended_code
        ),
        other => other.to_string(),
    }
}

/// A byte string bound as SQLite text (the statement wraps it in `CAST`).
pub(crate) struct Bytes<'a>(pub &'a [u8]);

impl ToSql for Bytes<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Blob(self.0)))
    }
}

/// A byte string that is `NULL` when empty (`nullStr`).
pub(crate) struct NullBytes<'a>(pub &'a [u8]);

impl ToSql for NullBytes<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        if self.0.is_empty() {
            Ok(ToSqlOutput::Borrowed(ValueRef::Null))
        } else {
            Ok(ToSqlOutput::Borrowed(ValueRef::Blob(self.0)))
        }
    }
}

type Metadata = BTreeMap<String, Vec<u8>>;

fn metadata_json(metadata: &Metadata) -> String {
    let members = metadata
        .iter()
        .map(|(key, value)| format!("{}:{}", json_string(key.as_bytes()), json_string(value)))
        .collect::<Vec<_>>();
    format!("{{{}}}", members.join(","))
}

fn redact_map(metadata: &Metadata) -> (Metadata, Vec<&'static str>) {
    let mut labels = Vec::new();
    let cleaned = metadata
        .iter()
        .map(|(key, value)| {
            let redacted = crate::pii::redact(value);
            labels.extend(redacted.labels);
            (key.clone(), redacted.bytes)
        })
        .collect();
    (cleaned, labels)
}

fn defaults(source_tool: &str) -> Metadata {
    [
        ("source_type", "direct"),
        ("source_tool", source_tool),
        ("source_uri", ""),
        ("authority", "direct"),
        ("confidence", "high"),
        ("verification_status", "unverified"),
        ("sensitivity", "internal"),
        ("sharing_level", "private"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.as_bytes().to_vec()))
    .collect()
}

/// `os.Getwd`: `$PWD` wins when it names the same directory as `.`.
fn getwd() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    if let Some(pwd) = std::env::var_os("PWD").map(std::path::PathBuf::from)
        && pwd.is_absolute()
        && let (Ok(logical), Ok(physical)) = (std::fs::metadata(&pwd), std::fs::metadata("."))
        && logical.dev() == physical.dev()
        && logical.ino() == physical.ino()
    {
        return Some(pwd);
    }
    std::env::current_dir().ok()
}

fn base_name(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.file_name()
        .map_or_else(|| b"/".to_vec(), |name| name.as_bytes().to_vec())
}

/// `ProjectScopeDetector.DetectActiveProject`.
pub(crate) fn active_project() -> Vec<u8> {
    let Some(cwd) = getwd() else {
        return b"default_project".to_vec();
    };
    for directory in cwd.ancestors() {
        if directory.join(".symmemory.toml").exists() || directory.join(".git").exists() {
            return base_name(directory);
        }
    }
    base_name(&cwd)
}

fn sha256_hex(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// One row to insert; the shape `MemoryInsertArgs` produces.
#[derive(Clone, Copy)]
struct Row<'a> {
    id: &'a str,
    content: &'a [u8],
    scope: &'a str,
    metadata: &'a Metadata,
    vector: &'a [f32],
    source: &'a str,
    model: &'a str,
    created_at: DateTime<Utc>,
    valid_from: DateTime<Utc>,
    author: &'a [u8],
    superseded_by: &'a str,
    quantize_binary: bool,
}

fn insert(conn: &Connection, row: &Row<'_>) -> Result<(), String> {
    let lsh = crate::lsh::compute_lsh(row.vector)?;
    let binary = (row.quantize_binary && !row.vector.is_empty())
        .then(|| crate::cli_write::sign_bits(row.vector));
    let dim = i64::try_from(row.vector.len()).unwrap_or(768);
    let now = crate::gotime::format(Utc::now());
    let superseded = (!row.superseded_by.is_empty()).then_some(row.superseded_by);
    conn.execute(
        "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,content_hash,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,consolidation_status,consolidated_into_id,importance,valid_from,valid_to,superseded_by,tier,expires_at,access_count,last_access,prev_access,review_status,kind,decay_factor,retired_at) VALUES(?,CAST(? AS TEXT),?,?,?,?,?,?,?,?,?,?,?,?,CAST(? AS TEXT),CAST(? AS TEXT),'','','raw',NULL,0,?,NULL,?,'long_term',NULL,1,NULL,NULL,'approved','',1,NULL)",
        params![
            row.id,
            Bytes(row.content),
            row.scope,
            metadata_json(row.metadata),
            crate::cli_write::embedding_json(row.vector),
            binary,
            dim,
            row.source,
            row.model,
            "",
            sha256_hex(row.content),
            lsh,
            crate::gotime::format(row.created_at),
            now,
            Bytes(row.author),
            Bytes(row.author),
            crate::gotime::format(row.valid_from),
            superseded,
        ],
    )
    .map_err(|error| go_sqlite_error(&error))?;
    audit(conn, "set", row.id, row.scope, row.author, "");
    Ok(())
}

/// `LogAudit` with byte-exact actor text; failures are ignored like in Go.
pub(crate) fn audit(
    conn: &Connection,
    action: &str,
    memory_id: &str,
    scope: &str,
    actor: &[u8],
    detail: &str,
) {
    let nonempty = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    let _ = conn.execute(
        "INSERT INTO audit_log(id,action,memory_id,scope,session,actor,detail,created_at) VALUES(?,?,?,?,?,CAST(? AS TEXT),?,?)",
        params![
            crate::write::new_uuid(),
            action,
            nonempty(memory_id),
            nonempty(scope),
            Option::<String>::None,
            NullBytes(actor),
            nonempty(detail),
            crate::gotime::format(Utc::now()),
        ],
    );
}

struct Written {
    id: String,
    scope: String,
    content: Vec<u8>,
    metadata: Metadata,
}

fn metadata_of(row: &SearchRow) -> Metadata {
    row.metadata
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.as_str().unwrap_or_default().as_bytes().to_vec(),
            )
        })
        .collect()
}

fn f32_detail(value: f32) -> String {
    crate::cli_write::f32_json(value)
}

fn supersede_detail(resolution: &Resolution) -> String {
    format!(
        "{{\"winner_id\":{},\"winner_actor\":{},\"loser_actor\":{},\"rule\":{},\"similarity\":{}}}",
        json_string(resolution.winner_id.as_bytes()),
        json_string(&resolution.winner_actor),
        json_string(&resolution.loser_actor),
        json_string(resolution.rule.as_bytes()),
        f32_detail(resolution.similarity),
    )
}

fn pending_detail(resolution: &Resolution) -> String {
    format!(
        "{{\"candidate_id\":{},\"candidate_actor\":{},\"similarity\":{}}}",
        json_string(resolution.candidate.id.as_bytes()),
        json_string(resolution.candidate.created_by.as_bytes()),
        f32_detail(resolution.similarity),
    )
}

struct Pipeline<'a> {
    conn: &'a Connection,
    request: &'a CliSet,
    generator: &'a EmbeddingGenerator,
    checker: bool,
    patterns: BTreeSet<&'static str>,
}

impl Pipeline<'_> {
    /// `storeWithConflictCheck` (or the plain `SaveMemory` without a
    /// checker). Returns the stored memory and whether the write was
    /// deduplicated onto an existing row.
    fn save(
        &self,
        row: &Row<'_>,
        log: &mut LogSink<'_>,
    ) -> Result<(Option<SearchRow>, String), String> {
        if !self.checker {
            insert(self.conn, row)?;
            return Ok((None, String::new()));
        }
        let hash = sha256_hex(row.content);
        let new = NewMemory {
            id: row.id,
            content: row.content,
            content_hash: &hash,
            scope: row.scope,
            embedding: row.vector,
            embedding_source: row.source,
            created_by: row.author,
            created_at: row.created_at,
            importance: 0.0,
        };
        let resolutions = match conflict::check(self.conn, &self.request.conflict, &new, self.request.prefilter, log) {
            Ok(resolutions) => resolutions,
            Err(error) => {
                log.warn(
                    "conflict check degraded; storing memory unchanged",
                    &[("error", &error)],
                );
                Vec::new()
            }
        };
        if resolutions.len() == 1 && resolutions[0].verdict == Verdict::Repeat {
            let existing = resolutions.into_iter().next().map(|found| found.candidate);
            return Ok((existing, String::new()));
        }
        let mut superseded_by = String::new();
        let mut contradictions = Vec::new();
        for resolution in &resolutions {
            match resolution.verdict {
                Verdict::Contradiction => {
                    if resolution.winner_id != row.id {
                        superseded_by.clone_from(&resolution.winner_id);
                    }
                    contradictions.push(resolution);
                }
                Verdict::Ambiguous => {
                    audit(
                        self.conn,
                        "conflict_pending",
                        row.id,
                        row.scope,
                        row.author,
                        &pending_detail(resolution),
                    );
                }
                Verdict::Repeat => {}
            }
        }
        let row = Row {
            superseded_by: &superseded_by,
            ..*row
        };
        insert(self.conn, &row)?;
        for resolution in contradictions {
            if resolution.winner_id == row.id {
                // The new fact wins: close the loser's validity window at the
                // winner's creation time.
                let updated = self.conn.execute(
                    "UPDATE memories SET superseded_by = ?, valid_to = ?, updated_at = ?, updated_by = CAST(? AS TEXT), updated_session = NULL WHERE id = ? AND (superseded_by IS NULL OR superseded_by = '')",
                    params![
                        row.id,
                        crate::gotime::format(row.created_at),
                        crate::gotime::format(Utc::now()),
                        NullBytes(row.author),
                        resolution.loser_id,
                    ],
                );
                if let Err(error) = updated {
                    log.warn(
                        "failed to mark memory superseded",
                        &[
                            ("loser", &resolution.loser_id),
                            ("error", &go_sqlite_error(&error)),
                        ],
                    );
                    continue;
                }
            }
            audit(
                self.conn,
                "supersede",
                &resolution.loser_id,
                row.scope,
                row.author,
                &supersede_detail(resolution),
            );
        }
        Ok((None, String::new()))
    }
}

impl Store {
    /// Runs the shipped CLI write: `SetGoverned` over `memory.Store`.
    ///
    /// # Errors
    /// Returns the shipped error text for a rejected scope, a failed insert
    /// or a governance step that no longer finds the row.
    #[allow(clippy::too_many_lines)]
    pub fn set_cli(
        &self,
        request: &CliSet,
        generator: &EmbeddingGenerator,
        log: &mut LogSink<'_>,
    ) -> Result<String, StoreError> {
        let conn = self.lock()?;
        let invalid = |message: String| StoreError::Invalid(message);

        // Prepare.
        let scope = if request.scope.is_empty() {
            "global".to_owned()
        } else if let Some(valid) = ["global", "project", "agent", "user", "session"]
            .iter()
            .find(|name| name.as_bytes() == request.scope.as_slice())
        {
            (*valid).to_owned()
        } else {
            return Err(invalid(format!(
                "invalid scope {}: must be one of global, project, agent, user, session",
                quote_bytes(&request.scope)
            )));
        };
        let mut pipeline = Pipeline {
            conn: &conn,
            request,
            generator,
            checker: request.conflict.enabled && !request.staged,
            patterns: BTreeSet::new(),
        };

        let redacted = crate::pii::redact(&request.content);
        for label in &redacted.labels {
            log.warn("PII redacted from memory content", &[("pattern", label)]);
            pipeline.patterns.insert(label);
        }
        let content = redacted.bytes;

        let mut user: Metadata = request
            .metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.as_bytes().to_vec()))
            .collect();
        if scope == "project" {
            user.insert("project_name".into(), active_project());
        }
        let mut metadata = defaults(SOURCE_TOOL);
        metadata.insert(
            "observed_at".into(),
            Utc::now()
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
                .into_bytes(),
        );
        metadata.extend(user);
        let (metadata, labels) = redact_map(&metadata);
        for label in labels {
            log.warn("PII redacted from memory metadata", &[("pattern", label)]);
            pipeline.patterns.insert(label);
        }
        let valid_from = Utc::now();

        // Store.
        let id = crate::write::new_uuid();
        let embedding = generator.generate_bytes(&content);
        let model = if embedding.source == "ollama" {
            generator.model()
        } else {
            ""
        };
        let created_at = Utc::now();
        let row = Row {
            id: &id,
            content: &content,
            scope: &scope,
            metadata: &metadata,
            vector: &embedding.vector,
            source: &embedding.source,
            model,
            created_at,
            valid_from,
            author: &request.author,
            superseded_by: "",
            quantize_binary: request.quantize_binary,
        };
        let (existing, _) = pipeline
            .save(&row, log)
            .map_err(|error| invalid(format!("failed to save memory: {error}")))?;
        let stored = existing.map_or_else(
            || Written {
                id: id.clone(),
                scope: scope.clone(),
                content: content.clone(),
                metadata: metadata.clone(),
            },
            |found| {
                log.info(
                    "conflict: duplicate write deduplicated",
                    &[("id", &found.id)],
                );
                Written {
                    metadata: metadata_of(&found),
                    content: found.content.as_bytes().to_vec(),
                    scope: found.scope.clone(),
                    id: found.id,
                }
            },
        );

        crate::cli_entity::link_bytes(&conn, &stored.id, &request.entities, &request.author);

        pipeline.extract(&stored, log);

        if !pipeline.patterns.is_empty() {
            let detail = pipeline
                .patterns
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join(",");
            audit(
                &conn,
                "redaction",
                &stored.id,
                &stored.scope,
                &request.author,
                &detail,
            );
        }

        // SetGoverned.
        let affected = conn
            .execute(
                "UPDATE memories SET kind = ?, updated_at = ? WHERE id = ?",
                params![request.kind, crate::gotime::format(Utc::now()), stored.id],
            )
            .map_err(|error| invalid(go_sqlite_error(&error)))?;
        if affected == 0 {
            return Err(invalid(format!("memory not found: {}", stored.id)));
        }
        if request.staged {
            let affected = conn
                .execute(
                    "UPDATE memories SET review_status = 'staged', updated_at = ? WHERE id = ?",
                    params![crate::gotime::format(Utc::now()), stored.id],
                )
                .map_err(|error| invalid(go_sqlite_error(&error)))?;
            if affected == 0 {
                return Err(invalid(format!("memory not found: {}", stored.id)));
            }
        }
        Ok(stored.id)
    }
}

impl Pipeline<'_> {
    /// Secondary-fact extraction (`PatternExtractor.ExtractFacts` and the
    /// per-fact save/evidence loop of `memory.Store`).
    fn extract(&mut self, primary: &Written, log: &mut LogSink<'_>) {
        for fact in extract::extract_facts(&primary.content) {
            let redacted = crate::pii::redact(fact.content.as_bytes());
            for label in &redacted.labels {
                log.warn(
                    "PII redacted from extracted fact content",
                    &[("pattern", label)],
                );
                self.patterns.insert(label);
            }
            let clean = redacted.bytes;
            let raw_trigger = fact.metadata.get("raw_trigger").map_or("", String::as_str);
            let primary_text = String::from_utf8_lossy(&primary.content);
            if extract::is_duplicate_of_primary(
                &String::from_utf8_lossy(&clean),
                raw_trigger,
                &primary_text,
            ) {
                continue;
            }

            let sub_id = crate::write::new_uuid();
            let embedding = self.generator.generate_bytes(&clean);
            let mut sub: Metadata = fact
                .metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.as_bytes().to_vec()))
                .collect();
            if primary.scope == "project" {
                sub.insert(
                    "project_name".into(),
                    primary
                        .metadata
                        .get("project_name")
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            sub.insert("source_type".into(), b"extracted".to_vec());
            sub.insert("extraction_method".into(), b"pattern".to_vec());
            let parent_confidence = primary
                .metadata
                .get("confidence")
                .cloned()
                .unwrap_or_default();
            sub.insert("authority".into(), b"inferred".to_vec());
            sub.insert(
                "confidence".into(),
                if parent_confidence.is_empty() {
                    b"medium".to_vec()
                } else {
                    parent_confidence
                },
            );
            sub.insert("verification_status".into(), b"unverified".to_vec());
            for key in ["sensitivity", "sharing_level"] {
                if let Some(value) = primary.metadata.get(key)
                    && !value.is_empty()
                {
                    sub.insert(key.into(), value.clone());
                }
            }
            let (sub, labels) = redact_map(&sub);
            for label in labels {
                log.warn(
                    "PII redacted from extracted fact metadata",
                    &[("pattern", label)],
                );
                self.patterns.insert(label);
            }

            let now = Utc::now();
            let model = if embedding.source == "ollama" {
                self.generator.model()
            } else {
                ""
            };
            let row = Row {
                id: &sub_id,
                content: &clean,
                scope: &primary.scope,
                metadata: &sub,
                vector: &embedding.vector,
                source: &embedding.source,
                model,
                created_at: now,
                valid_from: now,
                author: &self.request.author,
                superseded_by: "",
                quantize_binary: self.request.quantize_binary,
            };
            match self.save(&row, log) {
                Ok((None, _)) => {}
                Ok((Some(found), _)) => {
                    log.info(
                        "conflict: extracted fact deduplicated",
                        &[("id", &found.id)],
                    );
                }
                Err(_) => continue,
            }
            if fact.evidence.is_empty() {
                continue;
            }
            let mut spans = Vec::with_capacity(fact.evidence.len());
            for mut span in fact.evidence {
                span.source = SourceRef {
                    id: primary.id.clone(),
                    kind: SOURCE_TOOL.to_owned(),
                };
                for text in [&mut span.evidence_text, &mut span.text] {
                    let redacted = crate::pii::redact(text.as_bytes());
                    *text = String::from_utf8_lossy(&redacted.bytes).into_owned();
                    for label in redacted.labels {
                        log.warn("PII redacted from evidence text", &[("pattern", label)]);
                        self.patterns.insert(label);
                    }
                }
                spans.push(span);
            }
            let _ = crate::evidence_store::save_memory_evidence(self.conn, &sub_id, &spans);
        }
    }
}
