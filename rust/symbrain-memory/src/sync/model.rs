//! Complete sync wire model, separate from the public lite Memory projection.

use super::SyncTime;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Every field in the frozen database Memory wire declaration.
///
/// The canonical Serde codec is not a complete Go JSON decoder: ordered
/// duplicates, folded names, null-retained fields and exact decode errors
/// remain transport admission gates. None of these types changes CLI routing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncMemory {
    pub id: String,
    pub content: String,
    pub scope: String,
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "empty_embedding")]
    pub embedding: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embedding_source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embedding_model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub embedding_quantization: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    pub created_at: SyncTime,
    pub updated_at: SyncTime,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_session: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated_session: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    pub consolidation_status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub consolidated_into_id: String,
    pub importance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<SyncTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<SyncTime>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub superseded_by: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub review_status: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub decay_factor: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<SyncTime>,
    pub tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<SyncTime>,
    pub access_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_access: Option<SyncTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_access: Option<SyncTime>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<SyncEvidence>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

fn empty_embedding(value: &Option<Vec<f32>>) -> bool {
    value.as_ref().is_none_or(Vec::is_empty)
}

/// Evidence can occur in a peer payload but Go's upsert does not persist it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncEvidence {
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
    pub created_at: SyncTime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeletedMemory {
    pub id: String,
    pub deleted_at: SyncTime,
}

/// A decoded relay blob. Base64 wire conversion belongs to the transport.
#[derive(Debug, Clone, Default)]
pub struct RelayBlob {
    pub id: String,
    pub updated_at: SyncTime,
    pub blob: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Changes {
    // A null element in Go's []*Memory panics in its runner. Transport must
    // refuse admission of that exceptional family until separately decided.
    pub memories: Option<Vec<SyncMemory>>,
    pub deleted: Option<Vec<DeletedMemory>>,
    pub server_time: SyncTime,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_cursor: String,
}

#[derive(Debug, Clone, Default)]
pub struct RelayChanges {
    pub blobs: Vec<RelayBlob>,
    pub server_time: SyncTime,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplyResult {
    pub applied: i64,
    pub skipped: i64,
    pub deleted: i64,
    pub skipped_invalid_scope: i64,
    pub skipped_invalid_id: i64,
}

impl<'de> Deserialize<'de> for ApplyResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = Option::<BTreeMap<String, serde_json::Value>>::deserialize(deserializer)?
            .unwrap_or_default();
        let count = |snake: &str, camel: &str| {
            fields
                .get(snake)
                .or_else(|| fields.get(camel))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default()
        };
        Ok(Self {
            applied: count("applied", "applied"),
            skipped: count("skipped", "skipped"),
            deleted: count("deleted", "deleted"),
            skipped_invalid_scope: count("skipped_invalid_scope", "skippedInvalidScope"),
            skipped_invalid_id: count("skipped_invalid_id", "skippedInvalidID"),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayPushResult {
    pub stored: i64,
    pub skipped: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<SyncMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<DeletedMemory>,
}
