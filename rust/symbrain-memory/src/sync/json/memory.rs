//! Full Memory field assignment, merge maps and retained slice backing slots.

use super::super::{DeletedMemory, SyncError, SyncEvidence, SyncMemory};
use super::{
    raw::Raw,
    strings::field,
    typed::{Decode, Slice, object},
};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub(super) struct Memory {
    pub value: SyncMemory,
    embedding: Slice<f32>,
    entities: Slice<String>,
    evidence: Slice<SyncEvidence>,
}
impl Memory {
    pub fn assign(
        &mut self,
        raw: Raw<'_>,
        decode: &mut Decode,
        path: &str,
    ) -> Result<(), SyncError> {
        let Some(members) = object(raw, decode, "db.Memory", path)? else {
            return Ok(());
        };
        let prefix = path.split_once('.').map_or("", |(_, path)| path);
        for (name, raw) in members {
            let name = field(&name);
            let path = if prefix.is_empty() {
                format!("Memory.{name}")
            } else {
                format!("Memory.{prefix}.{name}")
            };
            let row = &mut self.value;
            match name.as_str() {
                "id" => decode.string(raw, &mut row.id, &path)?,
                "content" => decode.string(raw, &mut row.content, &path)?,
                "scope" => decode.string(raw, &mut row.scope, &path)?,
                "metadata" => metadata(raw, &mut row.metadata, decode, &path)?,
                "embedding" => {
                    self.embedding.assign(
                        raw,
                        decode,
                        "[]float32",
                        &path,
                        |raw, value, decode| {
                            let mut wide = f64::from(*value);
                            decode.float(raw, &mut wide, true, &path);
                            #[allow(clippy::cast_possible_truncation)]
                            {
                                *value = wide as f32;
                            }
                            Ok(())
                        },
                    )?;
                    row.embedding = self.embedding.visible();
                }
                "embedding_source" => decode.string(raw, &mut row.embedding_source, &path)?,
                "embedding_model" => decode.string(raw, &mut row.embedding_model, &path)?,
                "embedding_quantization" => {
                    decode.string(raw, &mut row.embedding_quantization, &path)?
                }
                "content_hash" => decode.string(raw, &mut row.content_hash, &path)?,
                "created_at" => decode.clock(raw, &mut row.created_at)?,
                "updated_at" => decode.clock(raw, &mut row.updated_at)?,
                "created_by" => decode.string(raw, &mut row.created_by, &path)?,
                "updated_by" => decode.string(raw, &mut row.updated_by, &path)?,
                "created_session" => decode.string(raw, &mut row.created_session, &path)?,
                "updated_session" => decode.string(raw, &mut row.updated_session, &path)?,
                "entities" => {
                    self.entities.assign(
                        raw,
                        decode,
                        "[]string",
                        &path,
                        |raw, value, decode| decode.string(raw, value, &path),
                    )?;
                    row.entities = self.entities.visible().unwrap_or_default();
                }
                "consolidation_status" => {
                    decode.string(raw, &mut row.consolidation_status, &path)?
                }
                "consolidated_into_id" => {
                    decode.string(raw, &mut row.consolidated_into_id, &path)?
                }
                "importance" => decode.float(raw, &mut row.importance, false, &path),
                "valid_from" => optional_time(raw, &mut row.valid_from, decode)?,
                "valid_to" => optional_time(raw, &mut row.valid_to, decode)?,
                "superseded_by" => decode.string(raw, &mut row.superseded_by, &path)?,
                "kind" => decode.string(raw, &mut row.kind, &path)?,
                "review_status" => decode.string(raw, &mut row.review_status, &path)?,
                "decay_factor" => decode.float(raw, &mut row.decay_factor, false, &path),
                "retired_at" => optional_time(raw, &mut row.retired_at, decode)?,
                "tier" => decode.string(raw, &mut row.tier, &path)?,
                "expires_at" => optional_time(raw, &mut row.expires_at, decode)?,
                "access_count" => decode.integer(raw, &mut row.access_count, "int64", &path),
                "last_access" => optional_time(raw, &mut row.last_access, decode)?,
                "prev_access" => optional_time(raw, &mut row.prev_access, decode)?,
                "evidence" => {
                    self.evidence.assign(
                        raw,
                        decode,
                        "[]db.EvidenceSpan",
                        &path,
                        |raw, value, decode| evidence(raw, value, decode, &path),
                    )?;
                    row.evidence = self.evidence.visible().unwrap_or_default();
                }
                _ => {}
            }
        }
        Ok(())
    }
}
pub(super) fn optional_time(
    raw: Raw<'_>,
    value: &mut Option<super::super::SyncTime>,
    decode: &mut Decode,
) -> Result<(), SyncError> {
    if raw.null() {
        *value = None;
        return Ok(());
    }
    decode.clock(raw, value.get_or_insert_default())
}
fn metadata(
    raw: Raw<'_>,
    value: &mut Option<BTreeMap<String, String>>,
    decode: &mut Decode,
    path: &str,
) -> Result<(), SyncError> {
    if raw.null() {
        *value = None;
        return Ok(());
    }
    let Some(members) = object(raw, decode, "map[string]string", path)? else {
        return Ok(());
    };
    let map = value.get_or_insert_default();
    for (key, raw) in members {
        // A map element is freshly zeroed for every duplicate assignment;
        // null becomes empty string rather than retaining the previous value.
        let mut text = String::new();
        decode.string(raw, &mut text, path)?;
        map.insert(key, text);
    }
    Ok(())
}
pub(super) fn deleted(
    raw: Raw<'_>,
    value: &mut DeletedMemory,
    decode: &mut Decode,
    path: &str,
) -> Result<(), SyncError> {
    let Some(members) = object(raw, decode, "db.DeletedMemory", path)? else {
        return Ok(());
    };
    for (name, raw) in members {
        match field(&name).as_str() {
            "id" => decode.string(
                raw,
                &mut value.id,
                &format!(
                    "DeletedMemory.{}.id",
                    path.split_once('.').map_or("", |(_, path)| path)
                ),
            )?,
            "deleted_at" => decode.clock(raw, &mut value.deleted_at)?,
            _ => {}
        }
    }
    Ok(())
}
fn evidence(
    raw: Raw<'_>,
    value: &mut SyncEvidence,
    decode: &mut Decode,
    path: &str,
) -> Result<(), SyncError> {
    let Some(members) = object(raw, decode, "db.EvidenceSpan", path)? else {
        return Ok(());
    };
    for (name, raw) in members {
        let name = field(&name);
        let path = format!(
            "EvidenceSpan.{}.{name}",
            path.split_once('.').map_or("", |(_, path)| path)
        );
        match name.as_str() {
            "id" => decode.string(raw, &mut value.id, &path)?,
            "memory_id" => decode.string(raw, &mut value.memory_id, &path)?,
            "source_id" => decode.string(raw, &mut value.source_id, &path)?,
            "source_kind" => decode.string(raw, &mut value.source_kind, &path)?,
            "text" => decode.string(raw, &mut value.text, &path)?,
            "evidence_text" => decode.string(raw, &mut value.evidence_text, &path)?,
            "char_start" => decode.integer(raw, &mut value.char_start, "int", &path),
            "char_end" => decode.integer(raw, &mut value.char_end, "int", &path),
            "alignment_status" => decode.string(raw, &mut value.alignment_status, &path)?,
            "created_at" => decode.clock(raw, &mut value.created_at)?,
            _ => {}
        }
    }
    Ok(())
}
