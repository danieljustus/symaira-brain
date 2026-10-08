//! Grounded evidence persistence on the same connection as the memory store.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::evidence::Extraction;
use crate::{Store, StoreError};

/// Persisted evidence associated with a durable memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub id: String,
    pub memory_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub evidence_text: String,
    pub char_start: i64,
    pub char_end: i64,
    pub alignment_status: String,
    pub created_at: DateTime<Utc>,
}

impl Store {
    /// Saves strictly grounded extractions; invalid records are skipped.
    /// Like Go's non-transactional method, earlier records survive a later error.
    ///
    /// # Errors
    /// Returns a SQLite, entropy or connection-lock error.
    pub fn save_memory_evidence(
        &self,
        memory_id: &str,
        extractions: &[Extraction],
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        save_memory_evidence(&conn, memory_id, extractions)
    }

    /// Returns evidence in the Go contract's oldest-first order.
    ///
    /// # Errors
    /// Returns a SQLite, timestamp or connection-lock error.
    pub fn get_memory_evidence(&self, memory_id: &str) -> Result<Vec<EvidenceSpan>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at FROM memory_evidence WHERE memory_id=? ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([memory_id], |row| {
            let raw: String = row.get(9)?;
            let created_at = crate::gotime::parse(&raw).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid evidence timestamp",
                    )),
                )
            })?;
            Ok(EvidenceSpan {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                source_id: row.get(2)?,
                source_kind: row.get(3)?,
                text: row.get(4)?,
                evidence_text: row.get(5)?,
                char_start: row.get(6)?,
                char_end: row.get(7)?,
                alignment_status: row.get(8)?,
                created_at,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

/// Transactional counterpart for extraction, consolidation and import callers.
///
/// # Errors
/// Returns a SQLite or entropy error. The caller owns commit or rollback.
pub fn save_memory_evidence_tx(
    tx: &Transaction<'_>,
    memory_id: &str,
    extractions: &[Extraction],
) -> Result<(), StoreError> {
    save_memory_evidence(tx, memory_id, extractions)
}

/// Transfers evidence before consolidation deletes or archives the source memory.
///
/// # Errors
/// Returns a SQLite error. Empty or identical identifiers are no-ops, as in Go.
pub fn reparent_memory_evidence_tx(
    tx: &Transaction<'_>,
    old_id: &str,
    new_id: &str,
) -> Result<(), StoreError> {
    if old_id.is_empty() || new_id.is_empty() || old_id == new_id {
        return Ok(());
    }
    tx.execute(
        "UPDATE memory_evidence SET memory_id=? WHERE memory_id=?",
        params![new_id, old_id],
    )?;
    Ok(())
}

fn save_memory_evidence(
    conn: &Connection,
    memory_id: &str,
    extractions: &[Extraction],
) -> Result<(), StoreError> {
    let now = crate::gotime::format(Utc::now());
    for extraction in extractions {
        if extraction.validate().is_err() {
            continue;
        }
        conn.execute(
            "INSERT INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
            params![new_id()?, memory_id, extraction.source.id, extraction.source.kind, extraction.text,
                extraction.evidence_text, extraction.span.start, extraction.span.end, extraction.alignment_status, now],
        )?;
    }
    Ok(())
}

fn new_id() -> Result<String, StoreError> {
    use std::fmt::Write as _;
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| StoreError::Io(format!("evidence UUID: {error}")))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut id = String::with_capacity(36);
    for (index, byte) in bytes.into_iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            id.push('-');
        }
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}

#[cfg(test)]
mod tests;
