//! Go field order, nil slices, omitempty, HTML escaping and numeric encoding.

use super::{
    DeletedMemory, RelayBlob, RelayPayload, SyncError, SyncEvidence, SyncMemory, SyncTime,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Datelike;

pub(super) fn quote(text: &str) -> String {
    serde_json::to_string(text)
        .expect("UTF8 JSON string")
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn number(value: f64, narrow: Option<f32>) -> Result<String, SyncError> {
    if !value.is_finite() {
        return Err(SyncError(format!(
            "json: unsupported value: {}",
            if value.is_nan() {
                "NaN"
            } else if value.is_sign_negative() {
                "-Inf"
            } else {
                "+Inf"
            }
        )));
    }
    if value == 0.0 {
        return Ok(if value.is_sign_negative() { "-0" } else { "0" }.into());
    }
    let exponential = narrow.map_or_else(
        || value.abs() < 1e-6 || value.abs() >= 1e21,
        |value| value.abs() < 1e-6 || value.abs() >= 1e21,
    );
    if !exponential {
        return Ok(narrow.map_or_else(|| format!("{value}"), |value| format!("{value}")));
    }
    let raw = narrow.map_or_else(|| format!("{value:e}"), |value| format!("{value:e}"));
    let (mantissa, exponent) = raw.split_once('e').expect("scientific float format");
    let exponent: i32 = exponent.parse().expect("scientific exponent");
    Ok(format!("{mantissa}e{exponent:+}"))
}
fn clock(time: SyncTime) -> Result<String, SyncError> {
    if !(0..=9999).contains(&time.0.year()) {
        return Err(SyncError("json: error calling MarshalJSON for type time.Time: Time.MarshalJSON: year outside of range [0,9999]".into()));
    }
    Ok(quote(&time.wire()))
}
struct Object(Vec<String>);
impl Object {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn raw(&mut self, key: &str, value: String) {
        self.0.push(format!("{}:{value}", quote(key)));
    }
    fn string(&mut self, key: &str, text: &str) {
        self.raw(key, quote(text));
    }
    fn optional(&mut self, key: &str, text: &str) {
        if !text.is_empty() {
            self.string(key, text);
        }
    }
    fn time(&mut self, key: &str, time: Option<SyncTime>) -> Result<(), SyncError> {
        if let Some(time) = time {
            self.raw(key, clock(time)?);
        }
        Ok(())
    }
    fn finish(self) -> String {
        format!("{{{}}}", self.0.join(","))
    }
}

pub(super) fn memory(row: &SyncMemory) -> Result<String, SyncError> {
    let mut out = Object::new();
    out.string("id", &row.id);
    out.string("content", &row.content);
    out.string("scope", &row.scope);
    let metadata = if let Some(map) = &row.metadata {
        format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        "null".into()
    };
    out.raw("metadata", metadata);
    if let Some(vector) = &row.embedding {
        if !vector.is_empty() {
            out.raw(
                "embedding",
                format!(
                    "[{}]",
                    vector
                        .iter()
                        .map(|value| number(f64::from(*value), Some(*value)))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(",")
                ),
            );
        }
    }
    for (key, value) in [
        ("embedding_source", &row.embedding_source),
        ("embedding_model", &row.embedding_model),
        ("embedding_quantization", &row.embedding_quantization),
        ("content_hash", &row.content_hash),
    ] {
        out.optional(key, value);
    }
    out.time("created_at", Some(row.created_at))?;
    out.time("updated_at", Some(row.updated_at))?;
    for (key, value) in [
        ("created_by", &row.created_by),
        ("updated_by", &row.updated_by),
        ("created_session", &row.created_session),
        ("updated_session", &row.updated_session),
    ] {
        out.optional(key, value);
    }
    if !row.entities.is_empty() {
        out.raw(
            "entities",
            format!(
                "[{}]",
                row.entities
                    .iter()
                    .map(|s| quote(s))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
    }
    out.string("consolidation_status", &row.consolidation_status);
    out.optional("consolidated_into_id", &row.consolidated_into_id);
    out.raw("importance", number(row.importance, None)?);
    out.time("valid_from", row.valid_from)?;
    out.time("valid_to", row.valid_to)?;
    out.optional("superseded_by", &row.superseded_by);
    out.optional("kind", &row.kind);
    out.string("review_status", &row.review_status);
    if row.decay_factor != 0.0 {
        out.raw("decay_factor", number(row.decay_factor, None)?);
    }
    out.time("retired_at", row.retired_at)?;
    out.string("tier", &row.tier);
    out.time("expires_at", row.expires_at)?;
    out.raw("access_count", row.access_count.to_string());
    out.time("last_access", row.last_access)?;
    out.time("prev_access", row.prev_access)?;
    if !row.evidence.is_empty() {
        out.raw(
            "evidence",
            format!(
                "[{}]",
                row.evidence
                    .iter()
                    .map(evidence)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
        );
    }
    Ok(out.finish())
}
fn evidence(row: &SyncEvidence) -> Result<String, SyncError> {
    let mut out = Object::new();
    out.string("id", &row.id);
    out.string("memory_id", &row.memory_id);
    out.optional("source_id", &row.source_id);
    out.optional("source_kind", &row.source_kind);
    out.optional("text", &row.text);
    out.string("evidence_text", &row.evidence_text);
    out.raw("char_start", row.char_start.to_string());
    out.raw("char_end", row.char_end.to_string());
    out.string("alignment_status", &row.alignment_status);
    out.time("created_at", Some(row.created_at))?;
    Ok(out.finish())
}
pub(super) fn deleted(row: &DeletedMemory) -> Result<String, SyncError> {
    let mut out = Object::new();
    out.string("id", &row.id);
    out.time("deleted_at", Some(row.deleted_at))?;
    Ok(out.finish())
}
pub(super) fn apply(
    memories: &[SyncMemory],
    deletes: &[DeletedMemory],
) -> Result<Vec<u8>, SyncError> {
    let mut out = Object::new();
    out.raw(
        "memories",
        if memories.is_empty() {
            "null".into()
        } else {
            format!(
                "[{}]",
                memories
                    .iter()
                    .map(memory)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            )
        },
    );
    out.raw(
        "deleted",
        if deletes.is_empty() {
            "null".into()
        } else {
            format!(
                "[{}]",
                deletes
                    .iter()
                    .map(deleted)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            )
        },
    );
    Ok(out.finish().into_bytes())
}
pub(super) fn relay_payload(payload: &RelayPayload) -> Result<Vec<u8>, SyncError> {
    let mut out = Object::new();
    if let Some(row) = &payload.memory {
        out.raw("memory", memory(row)?);
    }
    if let Some(row) = &payload.deleted {
        out.raw("deleted", deleted(row)?);
    }
    Ok(out.finish().into_bytes())
}
pub(super) fn relay(blobs: &[RelayBlob]) -> Result<Vec<u8>, SyncError> {
    let mut encoded = Vec::with_capacity(blobs.len());
    for blob in blobs {
        let mut out = Object::new();
        out.string("id", &blob.id);
        out.time("updated_at", Some(blob.updated_at))?;
        if !blob.blob_present && blob.blob.is_empty() {
            out.raw("blob", "null".into());
        } else {
            out.string("blob", &STANDARD.encode(&blob.blob));
        }
        encoded.push(out.finish());
    }
    let mut out = Object::new();
    out.raw("blobs", format!("[{}]", encoded.join(",")));
    Ok(out.finish().into_bytes())
}
