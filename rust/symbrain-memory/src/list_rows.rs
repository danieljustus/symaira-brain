//! `memory list` rows in the shipped shape.
//!
//! The shipped CLI reads the 25-column "lite" projection and encodes it with
//! `encoding/json`, so the table and the JSON here are produced from the same
//! columns and in the same field order. Times are parsed with [`gotime`] so a
//! database written by either implementation reads the same.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row};

use crate::gojson::Members;

/// Column list of the shipped lite projection.
const LITE_COLUMNS: &str = "id, content, scope, metadata, created_at, updated_at, created_by, \
     updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, \
     importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, \
     prev_access, review_status, kind, decay_factor, retired_at";

/// Filter and ordering of the shipped `memory list` scan.
const LITE_FILTER: &str = "consolidation_status != 'archived' AND (tier != 'working' OR \
     expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND \
     retired_at IS NULL";

/// One row of `memory list`.
#[derive(Debug, Clone)]
pub struct MemoryListRow {
    /// Identifier.
    pub id: String,
    /// Content.
    pub content: String,
    /// Scope.
    pub scope: String,
    /// Metadata object, as stored.
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Creation timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Update timestamp.
    pub updated_at: Option<DateTime<Utc>>,
    /// Actor that created the memory.
    pub created_by: String,
    /// Actor that last updated it.
    pub updated_by: String,
    /// Session that created it.
    pub created_session: String,
    /// Session that last updated it.
    pub updated_session: String,
    /// Consolidation status.
    pub consolidation_status: String,
    /// Memory this one was consolidated into.
    pub consolidated_into_id: String,
    /// Importance weight.
    pub importance: f64,
    /// Validity window start.
    pub valid_from: Option<DateTime<Utc>>,
    /// Validity window end.
    pub valid_to: Option<DateTime<Utc>>,
    /// Memory that superseded this one.
    pub superseded_by: String,
    /// Working or long-term tier.
    pub tier: String,
    /// Expiry for working-tier memories.
    pub expires_at: Option<DateTime<Utc>>,
    /// Retrieval count.
    pub access_count: i64,
    /// Last retrieval.
    pub last_access: Option<DateTime<Utc>>,
    /// Retrieval before the most recent one.
    pub prev_access: Option<DateTime<Utc>>,
    /// Review status.
    pub review_status: String,
    /// Semantic kind.
    pub kind: String,
    /// Aging multiplier.
    pub decay_factor: f64,
    /// Retirement timestamp.
    pub retired_at: Option<DateTime<Utc>>,
}

impl MemoryListRow {
    /// Renders the row the way the shipped encoder does: field order, empty
    /// fields omitted, Go float and timestamp rendering.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        let mut members = Members::default();
        members.string("id", &self.id);
        members.string("content", &self.content);
        members.string("scope", &self.scope);
        members.raw("metadata", &render_map(&self.metadata));
        members.time("created_at", self.created_at);
        members.time("updated_at", self.updated_at);
        members.string_if_present("created_by", &self.created_by);
        members.string_if_present("updated_by", &self.updated_by);
        members.string_if_present("created_session", &self.created_session);
        members.string_if_present("updated_session", &self.updated_session);
        members.string("consolidation_status", &self.consolidation_status);
        members.string_if_present("consolidated_into_id", &self.consolidated_into_id);
        members.number("importance", self.importance);
        members.time_if_present("valid_from", self.valid_from);
        members.time_if_present("valid_to", self.valid_to);
        members.string_if_present("superseded_by", &self.superseded_by);
        members.string_if_present("kind", &self.kind);
        members.string("review_status", &self.review_status);
        if self.decay_factor != 0.0 {
            members.number("decay_factor", self.decay_factor);
        }
        members.time_if_present("retired_at", self.retired_at);
        members.string("tier", &self.tier);
        members.time_if_present("expires_at", self.expires_at);
        members.integer("access_count", self.access_count);
        members.time_if_present("last_access", self.last_access);
        members.time_if_present("prev_access", self.prev_access);
        members.finish()
    }
}

/// Renders a metadata object with sorted keys and string values.
fn render_map(map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut members = Members::default();
    let mut keys = map.keys().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        let value = map.get(key).unwrap_or(&serde_json::Value::Null);
        match value.as_str() {
            Some(text) => members.string(key, text),
            // The shipped struct maps strings only; a non-string value would
            // fail its unmarshal, so keep the raw JSON visible instead.
            None => members.raw(key, &value.to_string()),
        }
    }
    members.finish()
}

/// One row of `memory rules`.
#[derive(Debug, Clone)]
pub struct RuleRow {
    /// Identifier.
    pub id: String,
    /// Rule content.
    pub content: String,
    /// Scope.
    pub scope: String,
    /// Metadata object, as stored.
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Creation timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Update timestamp.
    pub updated_at: Option<DateTime<Utc>>,
    /// Actor that created the rule.
    pub created_by: String,
    /// Actor that last updated it.
    pub updated_by: String,
}

impl RuleRow {
    /// Renders the rule the way the shipped encoder does.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        let mut members = Members::default();
        members.string("id", &self.id);
        members.string("content", &self.content);
        members.string("scope", &self.scope);
        members.raw("metadata", &render_map(&self.metadata));
        members.time("created_at", self.created_at);
        members.time("updated_at", self.updated_at);
        members.string_if_present("created_by", &self.created_by);
        members.string_if_present("updated_by", &self.updated_by);
        members.finish()
    }
}

/// Reads the rows a `memory rules` invocation reports.
///
/// # Errors
/// Returns a SQLite error when the scan fails.
pub(crate) fn list_rules(conn: &Connection, scope: &str) -> rusqlite::Result<Vec<RuleRow>> {
    let sql = if scope.is_empty() {
        "SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by \
         FROM rules ORDER BY created_at DESC"
    } else {
        "SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by \
         FROM rules WHERE scope = ? ORDER BY created_at DESC"
    };
    let mut statement = conn.prepare(sql)?;
    let mut rows = if scope.is_empty() {
        statement.query([])?
    } else {
        statement.query([scope])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let metadata_text: String = row.get(3)?;
        result.push(RuleRow {
            id: row.get(0)?,
            content: row.get(1)?,
            scope: row.get(2)?,
            metadata: serde_json::from_str(&metadata_text).unwrap_or_default(),
            created_at: time(row, 4)?,
            updated_at: time(row, 5)?,
            created_by: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            updated_by: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        });
    }
    Ok(result)
}

/// Reads the rows a `memory list` invocation reports.
///
/// # Errors
/// Returns a SQLite error when the scan fails.
pub(crate) fn list_lite(
    conn: &Connection,
    scope: &str,
    limit: i64,
) -> rusqlite::Result<Vec<MemoryListRow>> {
    let sql = if scope.is_empty() {
        format!(
            "SELECT {LITE_COLUMNS} FROM memories WHERE {LITE_FILTER} \
             ORDER BY created_at DESC, id DESC LIMIT ?"
        )
    } else {
        format!(
            "SELECT {LITE_COLUMNS} FROM memories WHERE scope = ? AND {LITE_FILTER} \
             ORDER BY created_at DESC, id DESC LIMIT ?"
        )
    };
    let mut statement = conn.prepare(&sql)?;
    let mut rows = if scope.is_empty() {
        statement.query([limit])?
    } else {
        statement.query(rusqlite::params![scope, limit])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(scan_lite(row)?);
    }
    Ok(result)
}

fn scan_lite(row: &Row<'_>) -> rusqlite::Result<MemoryListRow> {
    let metadata_text: String = row.get(3)?;
    Ok(MemoryListRow {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        metadata: serde_json::from_str(&metadata_text).unwrap_or_default(),
        created_at: time(row, 4)?,
        updated_at: time(row, 5)?,
        created_by: row.get(6)?,
        updated_by: row.get(7)?,
        created_session: row.get(8)?,
        updated_session: row.get(9)?,
        consolidation_status: row.get(10)?,
        consolidated_into_id: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        importance: row.get(12)?,
        valid_from: time(row, 13)?,
        valid_to: time(row, 14)?,
        superseded_by: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
        tier: row.get(16)?,
        expires_at: time(row, 17)?,
        access_count: row.get(18)?,
        last_access: time(row, 19)?,
        prev_access: time(row, 20)?,
        review_status: row.get(21)?,
        kind: row.get(22)?,
        decay_factor: row.get(23)?,
        retired_at: time(row, 24)?,
    })
}

fn time(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let raw: Option<String> = row.get(index)?;
    match raw {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => crate::gotime::parse(&value).map_or_else(
            || {
                Err(rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unrecognized timestamp: {value}"),
                    )),
                ))
            },
            |parsed| Ok(Some(parsed)),
        ),
    }
}
