//! `memory search` rows in the shipped shape.
//!
//! Search hydrates the full memory row, scores it, and encodes the result with
//! `encoding/json`; the table and the JSON here are produced from the same
//! columns and in the same field order, so both match the shipped bytes. Times
//! are parsed with [`crate::gotime`] so a database written by either
//! implementation reads the same.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::Row;

use crate::gojson::{self, Members};

#[path = "cli_conflict_json.rs"]
mod conflict_json;

/// Column list of the shipped full-row projection (30 columns).
pub(crate) const SEARCH_COLUMNS: &str = "id, content, scope, metadata, embedding, \
     embedding_binary, embedding_source, embedding_model, embedding_quantization, created_at, \
     updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, \
     consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, \
     access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at";

/// One hydrated memory row from a vector search.
#[derive(Debug, Clone)]
pub struct SearchRow {
    /// Identifier.
    pub id: String,
    /// Content.
    pub content: String,
    /// Scope.
    pub scope: String,
    /// Metadata object, as stored.
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Stored embedding vector, used for scoring and never rendered.
    pub embedding: Vec<f32>,
    /// Sign-bit embedding, used by the Hamming prefilter and never rendered.
    pub embedding_binary: Option<Vec<u8>>,
    /// Embedding dimension as stored.
    pub embedding_dim: i64,
    /// Which embedding space this vector belongs to.
    pub embedding_source: String,
    /// Embedding model, empty for the hash fallback.
    pub embedding_model: String,
    /// Quantization level of the embedding space.
    pub embedding_quantization: String,
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
    /// Expiry of a working-tier memory.
    pub expires_at: Option<DateTime<Utc>>,
    /// Number of times retrieved.
    pub access_count: i64,
    /// Last retrieval timestamp.
    pub last_access: Option<DateTime<Utc>>,
    /// Retrieval timestamp before the most recent one.
    pub prev_access: Option<DateTime<Utc>>,
    /// `approved` or `staged`.
    pub review_status: String,
    /// Semantic kind.
    pub kind: String,
    /// Aging multiplier in `(0, 1]`.
    pub decay_factor: f64,
    /// When set, the memory is retired.
    pub retired_at: Option<DateTime<Utc>>,
}

impl SearchRow {
    /// Reads one full memory row in the shipped column order.
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let _: Option<Vec<u8>> = row.get(5)?;
        let embedding: Vec<f32> = crate::rows::json(row, 4)?;
        if embedding.iter().any(|value| !value.is_finite()) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Self::from_decoded(row, crate::rows::json(row, 3)?, embedding)
    }

    /// Conflict checks must not turn malformed stored fields into decisions.
    pub(crate) fn from_conflict_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        // scanMemory scans the complete SQL row in projection order before
        // populateMemoryFields decodes metadata, then embedding JSON. Keep
        // scalar and time failures ahead of either decoder, not only times.
        for index in 0..30 {
            match index {
                5 => { let _: Option<Vec<u8>> = row.get(index)?; }
                9 | 10 => conflict_time(row, index, true)?,
                18 | 19 | 22 | 24 | 25 | 29 => conflict_time(row, index, false)?,
                16 | 20 => { let _: Option<String> = row.get(index)?; }
                17 | 28 => { let _: f64 = row.get(index)?; }
                23 => { let _: i64 = row.get(index)?; }
                _ => { let _: String = row.get(index)?; }
            }
        }
        let raw: String = row.get(4)?;
        // Go validates metadata before decoding the embedding.
        let metadata = conflict_metadata(row, 3)?;
        let embedding = conflict_json::embedding(&raw)
            .map_err(|error| conflict_json::row_error(4, error))?;
        // Go's Memory.Importance and CompositeScore retain float64. Only the
        // final composite score and cosine/threshold comparisons narrow to f32.
        Self::from_decoded(row, metadata, embedding)
    }

    fn from_decoded(
        row: &Row<'_>,
        metadata: serde_json::Map<String, serde_json::Value>,
        embedding: Vec<f32>,
    ) -> rusqlite::Result<Self> {
        let embedding_binary: Option<Vec<u8>> = row.get(5)?;
        Ok(Self {
            id: row.get(0)?,
            content: row.get(1)?,
            scope: row.get(2)?,
            metadata,
            // The shipped full scan omits `embedding_dim` and derives the
            // length from the vector itself.
            embedding_dim: i64::try_from(embedding.len()).unwrap_or_default(),
            embedding,
            embedding_binary,
            embedding_source: row.get(6)?,
            embedding_model: row.get(7)?,
            embedding_quantization: row.get(8)?,
            created_at: crate::gotime::parse_optional(row.get::<_, Option<String>>(9)?.as_ref()),
            updated_at: crate::gotime::parse_optional(row.get::<_, Option<String>>(10)?.as_ref()),
            created_by: row.get(11)?,
            updated_by: row.get(12)?,
            created_session: row.get(13)?,
            updated_session: row.get(14)?,
            consolidation_status: row.get(15)?,
            // NULL-tolerant like the shipped scan, which reads these two into
            // `sql.NullString` and keeps the empty string when absent.
            consolidated_into_id: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
            importance: row.get(17)?,
            valid_from: crate::gotime::parse_optional(row.get::<_, Option<String>>(18)?.as_ref()),
            valid_to: crate::gotime::parse_optional(row.get::<_, Option<String>>(19)?.as_ref()),
            superseded_by: row.get::<_, Option<String>>(20)?.unwrap_or_default(),
            tier: row.get(21)?,
            expires_at: crate::gotime::parse_optional(row.get::<_, Option<String>>(22)?.as_ref()),
            access_count: row.get(23)?,
            last_access: crate::gotime::parse_optional(row.get::<_, Option<String>>(24)?.as_ref()),
            prev_access: crate::gotime::parse_optional(row.get::<_, Option<String>>(25)?.as_ref()),
            review_status: row.get(26)?,
            kind: row.get(27)?,
            decay_factor: row.get(28)?,
            retired_at: crate::gotime::parse_optional(row.get::<_, Option<String>>(29)?.as_ref()),
        })
    }

    /// Creation timestamp on a second's precision, as the table encoder writes
    /// it (`time.RFC3339`).
    #[must_use]
    pub fn created_at_seconds(&self) -> String {
        self.created_at
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default())
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// Applies the shipped response-boundary redaction to content and
    /// metadata.
    pub fn redact(&mut self) {
        self.content = crate::store::redact_text(&self.content);
        for value in self.metadata.values_mut() {
            crate::store::redact_value(value);
        }
    }

    /// Renders the memory the way the shipped struct marshals it.
    ///
    /// The shipped CLI clears the embedding (and the binary vector is never
    /// marshalled), so both are absent here; the field order, `omitempty`
    /// markers and number spellings follow the shipped struct.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        let mut members = Members::default();
        members.string("id", &self.id);
        members.string("content", &self.content);
        members.string("scope", &self.scope);
        members.raw("metadata", &metadata_json(&self.metadata));
        members.string_if_present("embedding_source", &self.embedding_source);
        members.string_if_present("embedding_model", &self.embedding_model);
        members.string_if_present("embedding_quantization", &self.embedding_quantization);
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

pub(crate) fn conflict_metadata(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<serde_json::Map<String, serde_json::Value>> {
    let raw: String = row.get(index)?;
    // Typed decoding checks even a malformed value replaced by a later key.
    let map = conflict_json::metadata(&raw).map_err(|error| conflict_json::row_error(index, error))?;
    Ok(map.into_iter().map(|(key, value)| {
        (key, serde_json::Value::String(value.unwrap_or_default()))
    }).collect())
}

pub(crate) fn conflict_time(row: &Row<'_>, index: usize, required: bool) -> rusqlite::Result<()> {
    let value: Option<String> = row.get(index)?;
    if value.as_ref().is_none_or(|value| !crate::cli_delete_admission::canonical_time(value))
        && (required || value.is_some())
    {
        let name = row.as_ref().column_name(index)?;
        let source = if value.is_some() { "string" } else { "<nil>" };
        return Err(conflict_json::row_error(index, format!(
            "sql: Scan error on column index {index}, name \"{name}\": unsupported Scan, storing driver.Value type {source} into type *time.Time"
        )));
    }
    Ok(())
}

/// One scored search result.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// The hydrated memory.
    pub memory: SearchRow,
    /// Composite similarity score.
    pub score: f32,
}

impl SearchHit {
    /// Applies the shipped response-boundary redaction to the result.
    pub fn redact(&mut self) {
        self.memory.redact();
    }

    /// Renders the result the way the shipped `SearchResult` marshals it.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        let mut members = Members::default();
        members.raw("memory", &self.memory.to_go_json());
        members.raw("similarity_score", &gojson::number_f32(self.score));
        members.finish()
    }
}

/// Renders a metadata object the way the shipped `map[string]string` encodes:
/// keys sorted, values as strings.
fn metadata_json(metadata: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut members = Members::default();
    for (name, value) in metadata {
        match value {
            serde_json::Value::String(text) => members.string(name, text),
            // The shipped struct declares `map[string]string`, so anything
            // else would fail its decoder. Keeping the stored bytes visible is
            // the honest alternative to silently dropping the field.
            other => members.raw(name, &gojson::render_value(other)),
        }
    }
    members.finish()
}
