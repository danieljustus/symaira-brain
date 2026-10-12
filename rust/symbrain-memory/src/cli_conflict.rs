//! Deterministic tiers of the frozen Go write checker; no verdict provider.

use super::{embedding_json, go_json, timestamp};
use crate::search_rows::{SEARCH_COLUMNS, SearchRow};
use crate::{GeneratedEmbedding, Store, StoreError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) struct Config {
    pub contradiction: f64,
    pub near_dup: f64,
    pub max_candidates: i64,
}

impl Config {
    #[allow(clippy::cast_possible_truncation)]
    fn thresholds(self) -> (f32, f32, usize) {
        let normalize = |value: f64, default| {
            if value <= 0.0 || value >= 1.0 { default } else { value }
        };
        (
            normalize(self.contradiction, 0.80) as f32,
            normalize(self.near_dup, 0.95) as f32,
            usize::try_from(self.max_candidates.max(0)).ok().filter(|n| *n > 0).unwrap_or(10),
        )
    }
}

pub(super) struct Candidate {
    id: String,
    actor: String,
    similarity: f32,
}

#[derive(Default)]
pub(super) enum Plan {
    #[default]
    None,
    Repeat(String),
    NearDup(Candidate),
    Ambiguous(Vec<Candidate>),
}

impl Store {
    /// Read-only admission for existing-store deterministic CLI writes.
    /// Unknown/malformed row representations and possible extraction stay on Go.
    #[must_use]
    pub fn direct_conflict_supported(path: &Path, scope: &str) -> bool {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .is_ok_and(|conn| admission(&conn, scope).is_ok())
    }
}

fn admission(conn: &Connection, scope: &str) -> Result<(), StoreError> {
    let scope = if scope.is_empty() { "global" } else { scope };
    let mut query = conn.prepare(&format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE scope=?"))?;
    for row in query.query_map([scope], SearchRow::from_conflict_row)? {
        let memory = row?;
        // A hash repeat returns stored content to the extraction stage, not
        // the submitted content. Do not accidentally widen that boundary.
        if !crate::direct_content_supported(&memory.content) {
            return Err(StoreError::Invalid("conflict row requires Go extraction".into()));
        }
    }
    Ok(())
}

pub(super) fn check(
    conn: &Connection,
    config: Config,
    id: &str,
    content: &str,
    scope: &str,
    embedding: &GeneratedEmbedding,
) -> Result<Plan, StoreError> {
    if embedding.vector.is_empty() || embedding.source.is_empty() {
        return Ok(Plan::None);
    }
    // The hash tier intentionally has no source/dimension/TTL/valid_to filter
    // and performs no access write. This is FindActiveDuplicate, not retrieval.
    let duplicate: Option<String> = conn.query_row(
        "SELECT id FROM memories WHERE content_hash=? AND scope=? AND consolidation_status != 'archived' AND review_status='approved' AND retired_at IS NULL AND (superseded_by IS NULL OR superseded_by='') ORDER BY created_at DESC LIMIT 1",
        params![crate::write::content_hash(content), scope], |row| row.get(0),
    ).optional().map_err(|error| lookup_error("duplicate lookup", StoreError::Sql(error)))?;
    if let Some(duplicate) = duplicate {
        let memory = conn.query_row(
            &format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE id=?"),
            [&duplicate], SearchRow::from_conflict_row,
        ).optional().map_err(|error| lookup_error("duplicate lookup", StoreError::Sql(error)))?;
        if let Some(memory) = memory {
            return Ok(Plan::Repeat(memory.id));
        }
    }
    let (contradiction, near_dup, limit) = config.thresholds();
    let hits = crate::retrieval::search_for_conflict(
        conn, &embedding.vector, &embedding.source, scope, limit,
    ).map_err(|error| lookup_error("candidate recall", error))?;
    let mut best: Option<Candidate> = None;
    let mut band = Vec::new();
    for hit in hits {
        let memory = hit.memory;
        if memory.id == id || !memory.superseded_by.is_empty() || memory.embedding.is_empty() {
            continue;
        }
        let similarity = crate::retrieval::cosine_similarity(&embedding.vector, &memory.embedding);
        let candidate = Candidate { id: memory.id, actor: memory.created_by, similarity };
        if similarity >= near_dup {
            if best.as_ref().is_none_or(|old| similarity > old.similarity) {
                best = Some(candidate);
            }
        } else if similarity >= contradiction {
            band.push(candidate);
        }
    }
    Ok(best.map_or_else(|| {
        if band.is_empty() { Plan::None } else { Plan::Ambiguous(band) }
    }, Plan::NearDup))
}

impl Plan {
    pub(super) fn before_save(&self, conn: &Connection, id: &str, scope: &str, actor: &str,
        diagnostic: &mut dyn FnMut(&str, &[(&str, &str)]),
    ) {
        if let Self::Ambiguous(band) = self {
            for candidate in band {
                if let (Ok(candidate_id), Ok(candidate_actor)) =
                    (go_json(&candidate.id), go_json(&candidate.actor))
                {
                    let number = embedding_json(&[candidate.similarity]);
                    let detail = format!(
                        "{{\"candidate_id\":{candidate_id},\"candidate_actor\":{candidate_actor},\"similarity\":{}}}",
                        &number[1..number.len() - 1],
                    );
                    if let Err(error) = audit_result(conn, "conflict_pending", id, scope, "", actor, &detail) {
                        diagnostic("WARN failed to write conflict_pending audit event", &[("error", &sql_error(&error))]);
                    }
                }
            }
        }
    }

    pub(super) fn after_save(
        &self, conn: &Connection, id: &str, scope: &str, actor: &str, created_at: &str,
        diagnostic: &mut dyn FnMut(&str, &[(&str, &str)]),
    ) {
        if let Self::NearDup(candidate) = self {
            // This is deliberately not a transaction with the INSERT or audit.
            // Go ignores a failed supersession, skips its audit and keeps the
            // winner. An ignored/missing loser still gets the resolution audit.
            let updated = conn.execute(
                "UPDATE memories SET superseded_by=?,valid_to=?,updated_at=?,updated_by=?,updated_session=? WHERE id=? AND (superseded_by IS NULL OR superseded_by='')",
                params![id, created_at, timestamp(), (!actor.is_empty()).then_some(actor), Option::<String>::None, candidate.id],
            );
            if let Err(error) = updated {
                diagnostic("WARN failed to mark memory superseded", &[("loser", &candidate.id), ("error", &sql_error(&error))]);
                return;
            }
            if let (Ok(winner), Ok(winner_actor), Ok(loser_actor)) =
                (go_json(&id), go_json(&actor), go_json(&candidate.actor))
            {
                let number = embedding_json(&[candidate.similarity]);
                let detail = format!(
                    "{{\"winner_id\":{winner},\"winner_actor\":{winner_actor},\"loser_actor\":{loser_actor},\"rule\":\"near_dup\",\"similarity\":{}}}",
                    &number[1..number.len() - 1],
                );
                if let Err(error) = audit_result(conn, "supersede", &candidate.id, scope, "", actor, &detail) {
                    diagnostic("WARN failed to write supersede audit event", &[("error", &sql_error(&error))]);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "cli_conflict_tests.rs"]
mod tests;

pub(super) fn audit_result(
    conn: &Connection, action: &str, id: &str, scope: &str,
    session: &str, author: &str, detail: &str,
) -> rusqlite::Result<()> {
    let nonempty = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    conn.execute("INSERT INTO audit_log(id,action,memory_id,scope,session,actor,detail,created_at) VALUES(?,?,?,?,?,?,?,?)",
        params![crate::write::new_uuid(), action, nonempty(id), nonempty(scope), nonempty(session), nonempty(author), nonempty(detail), timestamp()])?;
    Ok(())
}

fn lookup_error(stage: &str, error: StoreError) -> StoreError {
    let detail = match error {
        StoreError::Sql(error) => sql_error(&error),
        other => other.to_string(),
    };
    StoreError::Invalid(format!("conflict: {stage}: {detail}"))
}

fn sql_error(error: &rusqlite::Error) -> String {
    if let rusqlite::Error::FromSqlConversionFailure(_, _, cause) = error {
        return cause.to_string();
    }
    // modernc sqlite v1.60.1 conn.go errstrForDB: sqlite3_errstr,
    // optional distinct sqlite3_errmsg, extended code and BUSY suffix.
    let (code, message) = match error {
        rusqlite::Error::SqliteFailure(code, message) => (code.extended_code, message.as_deref()),
        rusqlite::Error::SqlInputError { error, msg, .. } => (error.extended_code, Some(msg.as_str())),
        other => return other.to_string(),
    };
    let primary = match code & 0xff {
        1 => "SQL logic error", 2 => "internal logic error", 3 => "access permission denied",
        4 => "query aborted", 5 => "database is locked", 6 => "database table is locked",
        7 => "out of memory", 8 => "attempt to write a readonly database", 9 => "interrupted",
        10 => "disk I/O error", 11 => "database disk image is malformed", 12 => "unknown operation",
        13 => "database or disk is full", 14 => "unable to open database file", 15 => "locking protocol",
        16 => "unknown error", 17 => "database schema has changed", 18 => "string or blob too big",
        19 => "constraint failed", 20 => "datatype mismatch", 21 => "bad parameter or other API misuse",
        22 => "unknown error", 23 => "authorization denied", 24 => "unknown error",
        25 => "column index out of range", 26 => "file is not a database",
        27 => "notification message", 28 => "warning message", _ => "unknown error",
    };
    let detail = message.filter(|message| *message != primary)
        .map_or_else(String::new, |message| format!(": {message}"));
    let busy = if code == 5 { " (SQLITE_BUSY)" } else { "" };
    format!("{primary}{detail} ({code}){busy}")
}
