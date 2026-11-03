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
    /// The column held JSON `null` (a nil map), rendered as `null`.
    pub metadata_null: bool,
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
    /// Linked entity names (single-row loads only), `entities,omitempty`.
    pub entities: Vec<String>,
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
    /// Reads one full memory row in the shipped column order with the
    /// shipped `scanMemory` conversions and diagnostics.
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        use crate::goscan::{blob, float, int, null_text, null_time, raw, text, time};
        let id = text(row, 0, "id")?;
        let content = text(row, 1, "content")?;
        let scope = text(row, 2, "scope")?;
        let metadata_bytes = raw(row, 3, "metadata")?;
        let embedding_bytes = raw(row, 4, "embedding")?;
        let embedding_binary = blob(row, 5, "embedding_binary")?;
        let embedding_source = text(row, 6, "embedding_source")?;
        let embedding_model = text(row, 7, "embedding_model")?;
        let embedding_quantization = text(row, 8, "embedding_quantization")?;
        let created_at = time(row, 9, "created_at")?;
        let updated_at = time(row, 10, "updated_at")?;
        let created_by = text(row, 11, "created_by")?;
        let updated_by = text(row, 12, "updated_by")?;
        let created_session = text(row, 13, "created_session")?;
        let updated_session = text(row, 14, "updated_session")?;
        let consolidation_status = text(row, 15, "consolidation_status")?;
        let consolidated_into_id = null_text(row, 16, "consolidated_into_id")?;
        let importance = float(row, 17, "importance")?;
        let valid_from = null_time(row, 18, "valid_from")?;
        let valid_to = null_time(row, 19, "valid_to")?;
        let superseded_by = null_text(row, 20, "superseded_by")?;
        let tier = text(row, 21, "tier")?;
        let expires_at = null_time(row, 22, "expires_at")?;
        let access_count = int(row, 23, "access_count")?;
        let last_access = null_time(row, 24, "last_access")?;
        let prev_access = null_time(row, 25, "prev_access")?;
        let review_status = text(row, 26, "review_status")?;
        let kind = text(row, 27, "kind")?;
        let decay_factor = float(row, 28, "decay_factor")?;
        let retired_at = null_time(row, 29, "retired_at")?;
        let (metadata, metadata_null) = crate::goscan::decode_metadata(&metadata_bytes)?;
        let embedding = crate::goscan::decode_embedding(&embedding_bytes)?;
        Ok(Self {
            id,
            content,
            scope,
            metadata,
            metadata_null,
            // The shipped full scan omits `embedding_dim` and derives the
            // length from the vector itself.
            embedding_dim: i64::try_from(embedding.len()).unwrap_or_default(),
            embedding,
            embedding_binary,
            embedding_source,
            embedding_model,
            embedding_quantization,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            created_by,
            updated_by,
            created_session,
            updated_session,
            entities: Vec::new(),
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

    /// Applies the complete shipped `security.Redact` guard (HTTP response
    /// boundary and sync ingestion) to content and metadata values.
    pub fn redact_pii(&mut self) {
        self.content = crate::pii::redact(&self.content);
        for value in self.metadata.values_mut() {
            if let serde_json::Value::String(text) = value {
                *text = crate::pii::redact(text);
            }
        }
    }

    /// Renders the memory the way the shipped struct marshals it.
    ///
    /// The shipped CLI clears the embedding (and the binary vector is never
    /// marshalled), so both are absent here; the field order, `omitempty`
    /// markers and number spellings follow the shipped struct.
    #[must_use]
    pub fn to_go_json(&self) -> String {
        self.render(false)
    }

    /// Like [`Self::to_go_json`] but keeps the stored embedding, which the
    /// shipped HTTP responses marshal between `metadata` and
    /// `embedding_source` (`embedding,omitempty`).
    #[must_use]
    pub fn to_go_json_embedded(&self) -> String {
        self.render(true)
    }

    fn render(&self, embedding: bool) -> String {
        let mut members = Members::default();
        members.string("id", &self.id);
        members.string("content", &self.content);
        members.string("scope", &self.scope);
        members.raw(
            "metadata",
            &if self.metadata_null {
                "null".to_owned()
            } else {
                metadata_json(&self.metadata)
            },
        );
        if embedding && !self.embedding.is_empty() {
            let vector = self
                .embedding
                .iter()
                .map(|v| gojson::number_f32(*v))
                .collect::<Vec<_>>()
                .join(",");
            members.raw("embedding", &format!("[{vector}]"));
        }
        members.string_if_present("embedding_source", &self.embedding_source);
        members.string_if_present("embedding_model", &self.embedding_model);
        members.string_if_present("embedding_quantization", &self.embedding_quantization);
        members.time("created_at", self.created_at);
        members.time("updated_at", self.updated_at);
        members.string_if_present("created_by", &self.created_by);
        members.string_if_present("updated_by", &self.updated_by);
        members.string_if_present("created_session", &self.created_session);
        members.string_if_present("updated_session", &self.updated_session);
        if !self.entities.is_empty() {
            let names = self
                .entities
                .iter()
                .map(|name| gojson::quote(name))
                .collect::<Vec<_>>();
            members.raw("entities", &format!("[{}]", names.join(",")));
        }
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
