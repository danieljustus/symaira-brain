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
pub(crate) const LITE_COLUMNS: &str = "id, content, scope, metadata, created_at, updated_at, created_by, \
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
    /// The column held JSON `null` (a nil map), rendered as `null`.
    pub metadata_null: bool,
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
    /// Applies the complete shipped `security.Redact` guard to content and
    /// metadata values (the HTTP response boundary).
    pub fn redact_pii(&mut self) {
        self.content = crate::pii::redact(&self.content);
        for value in self.metadata.values_mut() {
            if let serde_json::Value::String(text) = value {
                *text = crate::pii::redact(text);
            }
        }
    }

    /// Renders the row the way the shipped encoder does: field order, empty
    /// fields omitted, Go float and timestamp rendering.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        let mut members = Members::default();
        members.string("id", &self.id);
        members.string("content", &self.content);
        members.string("scope", &self.scope);
        members.raw(
            "metadata",
            &render_metadata(&self.metadata, self.metadata_null),
        );
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

/// A nil map renders as `null`, any decoded object as an object.
fn render_metadata(map: &serde_json::Map<String, serde_json::Value>, null: bool) -> String {
    if null {
        "null".to_owned()
    } else {
        render_map(map)
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
    /// The column held JSON `null` (a nil map), rendered as `null`.
    pub metadata_null: bool,
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
        members.raw(
            "metadata",
            &render_metadata(&self.metadata, self.metadata_null),
        );
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
        use crate::goscan::{decode_metadata, raw, text, time};
        let id = text(row, 0, "id")?;
        let content = text(row, 1, "content")?;
        let scope = text(row, 2, "scope")?;
        let metadata_bytes = raw(row, 3, "metadata")?;
        let created_at = time(row, 4, "created_at")?;
        let updated_at = time(row, 5, "updated_at")?;
        let created_by = text(row, 6, "created_by")?;
        let updated_by = text(row, 7, "updated_by")?;
        let (metadata, metadata_null) = decode_metadata(&metadata_bytes)?;
        result.push(RuleRow {
            id,
            content,
            scope,
            metadata,
            metadata_null,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            created_by,
            updated_by,
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

pub(crate) fn scan_lite(row: &Row<'_>) -> rusqlite::Result<MemoryListRow> {
    use crate::goscan::{decode_metadata, float, int, null_text, null_time, raw, text, time};
    let id = text(row, 0, "id")?;
    let content = text(row, 1, "content")?;
    let scope = text(row, 2, "scope")?;
    let metadata_bytes = raw(row, 3, "metadata")?;
    let created_at = time(row, 4, "created_at")?;
    let updated_at = time(row, 5, "updated_at")?;
    let created_by = text(row, 6, "created_by")?;
    let updated_by = text(row, 7, "updated_by")?;
    let created_session = text(row, 8, "created_session")?;
    let updated_session = text(row, 9, "updated_session")?;
    let consolidation_status = text(row, 10, "consolidation_status")?;
    let consolidated_into_id = null_text(row, 11, "consolidated_into_id")?;
    let importance = float(row, 12, "importance")?;
    let valid_from = null_time(row, 13, "valid_from")?;
    let valid_to = null_time(row, 14, "valid_to")?;
    let superseded_by = null_text(row, 15, "superseded_by")?;
    let tier = text(row, 16, "tier")?;
    let expires_at = null_time(row, 17, "expires_at")?;
    let access_count = int(row, 18, "access_count")?;
    let last_access = null_time(row, 19, "last_access")?;
    let prev_access = null_time(row, 20, "prev_access")?;
    let review_status = text(row, 21, "review_status")?;
    let kind = text(row, 22, "kind")?;
    let decay_factor = float(row, 23, "decay_factor")?;
    let retired_at = null_time(row, 24, "retired_at")?;
    let (metadata, metadata_null) = decode_metadata(&metadata_bytes)?;
    Ok(MemoryListRow {
        id,
        content,
        scope,
        metadata,
        metadata_null,
        created_at: Some(created_at),
        updated_at: Some(updated_at),
        created_by,
        updated_by,
        created_session,
        updated_session,
        consolidation_status,
        consolidated_into_id,
        importance,
        valid_from,
        valid_to,
        superseded_by,
        tier,
        expires_at,
        access_count,
        last_access,
        prev_access,
        review_status,
        kind,
        decay_factor,
        retired_at,
    })
}
