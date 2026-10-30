//! The memories and tombstones of a `/api/sync/changes` reply, decoded the
//! way the shipped `db.Memory` / `db.DeletedMemory` decoders would.
//!
//! Only entries whose every field has a proven reading are claimed; any other
//! shape (wrong types, case-folded or duplicate keys, offsets in a time
//! spelling, embeddings, ...) returns `None` and the invocation stays on the
//! shipped client, which then reports its own decode error or applies it.

use super::sync_decode::{fold, parse_server_time};
use chrono::{DateTime, Utc};
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use symbrain_memory::{PulledDelete, PulledMemory};

/// A JSON object as ordered key/raw-value pairs, duplicates included.
struct RawPairs(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for RawPairs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PairsVisitor;
        impl<'de> Visitor<'de> for PairsVisitor {
            type Value = RawPairs;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawPairs, A::Error> {
                let mut pairs = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    pairs.push((key, map.next_value::<Box<RawValue>>()?));
                }
                Ok(RawPairs(pairs))
            }
        }
        deserializer.deserialize_map(PairsVisitor)
    }
}

/// The JSON names of `db.Memory` (and of `[]EvidenceSpan`, which the sync
/// path decodes and never stores).
const MEMORY_KEYS: [&str; 31] = [
    "id",
    "content",
    "scope",
    "metadata",
    "embedding",
    "embedding_source",
    "embedding_model",
    "embedding_quantization",
    "content_hash",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "created_session",
    "updated_session",
    "entities",
    "consolidation_status",
    "consolidated_into_id",
    "importance",
    "valid_from",
    "valid_to",
    "superseded_by",
    "kind",
    "review_status",
    "decay_factor",
    "retired_at",
    "tier",
    "expires_at",
    "access_count",
    "last_access",
    "prev_access",
];
const DELETE_KEYS: [&str; 2] = ["id", "deleted_at"];

/// Splits an object into its known `(key, raw text)` pairs; a known key spelled
/// with another case or repeated, or a non-object, is not claimed.
fn object(raw: &RawValue, known: &[&str]) -> Option<Vec<(String, String)>> {
    if !raw.get().starts_with('{') {
        return None;
    }
    let RawPairs(pairs) = serde_json::from_str(raw.get()).ok()?;
    let mut seen: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    for (key, value) in pairs {
        let folded = fold(&key);
        if !known.contains(&folded.as_str()) {
            continue;
        }
        if folded != key || seen.contains(&key) {
            return None;
        }
        seen.push(key.clone());
        kept.push((key, value.get().to_owned()));
    }
    Some(kept)
}

/// A field value without a proven reading.
struct Invalid;

/// A JSON string without NUL: SQLite text with an embedded NUL is not claimed.
fn string(raw: &str) -> Option<String> {
    let value: String = serde_json::from_str(raw).ok()?;
    (!value.contains('\0')).then_some(value)
}

fn text(raw: &str) -> Option<String> {
    match raw.as_bytes().first()? {
        b'n' => Some(String::new()),
        b'"' => string(raw),
        _ => None,
    }
}

fn time(raw: &str) -> Option<DateTime<Utc>> {
    match raw.as_bytes().first()? {
        b'n' => Some(symbrain_memory::zero_time()),
        b'"' => parse_server_time(&string(raw)?),
        _ => None,
    }
}

/// `null` is an absent pointer; anything else must be a time.
fn optional_time(raw: &str) -> Result<Option<DateTime<Utc>>, Invalid> {
    match raw.as_bytes().first() {
        Some(b'n') => Ok(None),
        _ => time(raw).map(Some).ok_or(Invalid),
    }
}

/// A JSON number read as `float64`: finite and syntactically a number.
fn float(raw: &str) -> Option<f64> {
    match raw.as_bytes().first()? {
        b'n' => Some(0.0),
        b'-' | b'0'..=b'9' => raw.parse::<f64>().ok().filter(|value| value.is_finite()),
        _ => None,
    }
}

/// A JSON number read as `int64`: an integer literal in range.
fn integer(raw: &str) -> Option<i64> {
    match raw.as_bytes().first()? {
        b'n' => Some(0),
        b'-' | b'0'..=b'9' => raw
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_digit())
            .then(|| raw.parse::<i64>().ok())?,
        _ => None,
    }
}

/// `map[string]string`: `null` is a nil map; every value must be a string.
fn metadata(raw: &str) -> Result<Option<BTreeMap<String, String>>, Invalid> {
    match raw.as_bytes().first() {
        Some(b'n') => Ok(None),
        Some(b'{') => {
            let RawPairs(pairs) = serde_json::from_str(raw).map_err(|_| Invalid)?;
            let mut map = BTreeMap::new();
            for (key, value) in pairs {
                if !value.get().starts_with('"') || key.contains('\0') {
                    return Err(Invalid);
                }
                map.insert(key, string(value.get()).ok_or(Invalid)?);
            }
            Ok(Some(map))
        }
        _ => Err(Invalid),
    }
}

/// `[]float32`: absent/`null` is nil, `[]` empty; vectors are not claimed.
fn embedding(raw: &str) -> Result<Option<Vec<f32>>, Invalid> {
    match raw.as_bytes().first() {
        Some(b'n') => Ok(None),
        Some(b'[') => {
            let items: Vec<Box<RawValue>> = serde_json::from_str(raw).map_err(|_| Invalid)?;
            if items.is_empty() {
                Ok(Some(Vec::new()))
            } else {
                Err(Invalid)
            }
        }
        _ => Err(Invalid),
    }
}

/// A field the sync path decodes and never stores: `null` or `[]` (or, for
/// `entities`, strings) only.
fn ignored_list(raw: &str, strings_only: bool) -> bool {
    match raw.as_bytes().first() {
        Some(b'n') => true,
        Some(b'[') => {
            let Ok(items) = serde_json::from_str::<Vec<Box<RawValue>>>(raw) else {
                return false;
            };
            items.is_empty() || strings_only && items.iter().all(|item| item.get().starts_with('"'))
        }
        _ => false,
    }
}

pub(super) fn memory(raw: &RawValue) -> Option<PulledMemory> {
    let mut keys = MEMORY_KEYS.to_vec();
    keys.push("evidence");
    let mut memory = PulledMemory::zero();
    for (key, value) in object(raw, &keys)? {
        let value = value.as_str();
        match key.as_str() {
            "id" => memory.id = text(value)?,
            "content" => memory.content = text(value)?,
            "scope" => memory.scope = text(value)?,
            "metadata" => memory.metadata = metadata(value).ok()?,
            "embedding" => memory.embedding = embedding(value).ok()?,
            "embedding_source" => memory.embedding_source = text(value)?,
            "embedding_model" => memory.embedding_model = text(value)?,
            "embedding_quantization" => memory.embedding_quantization = text(value)?,
            "content_hash" => memory.content_hash = text(value)?,
            "created_at" => memory.created_at = time(value)?,
            "updated_at" => memory.updated_at = time(value)?,
            "created_by" => memory.created_by = text(value)?,
            "updated_by" => memory.updated_by = text(value)?,
            "created_session" => memory.created_session = text(value)?,
            "updated_session" => memory.updated_session = text(value)?,
            "entities" => {
                if !ignored_list(value, true) {
                    return None;
                }
            }
            "evidence" => {
                if !ignored_list(value, false) {
                    return None;
                }
            }
            "consolidation_status" => memory.consolidation_status = text(value)?,
            "consolidated_into_id" => memory.consolidated_into_id = text(value)?,
            "importance" => memory.importance = float(value)?,
            "valid_from" => memory.valid_from = optional_time(value).ok()?,
            "valid_to" => memory.valid_to = optional_time(value).ok()?,
            "superseded_by" => memory.superseded_by = text(value)?,
            "kind" => memory.kind = text(value)?,
            "review_status" => memory.review_status = text(value)?,
            "decay_factor" => memory.decay_factor = float(value)?,
            "retired_at" => memory.retired_at = optional_time(value).ok()?,
            "tier" => memory.tier = text(value)?,
            "expires_at" => memory.expires_at = optional_time(value).ok()?,
            "access_count" => memory.access_count = integer(value)?,
            "last_access" => memory.last_access = optional_time(value).ok()?,
            "prev_access" => memory.prev_access = optional_time(value).ok()?,
            _ => return None,
        }
    }
    // An empty id would address no memory at all; the shipped client decides.
    (!memory.id.is_empty()).then_some(memory)
}

pub(super) fn tombstone(raw: &RawValue) -> Option<PulledDelete> {
    let mut delete = PulledDelete {
        id: String::new(),
        deleted_at: symbrain_memory::zero_time(),
    };
    for (key, value) in object(raw, &DELETE_KEYS)? {
        match key.as_str() {
            "id" => delete.id = text(&value)?,
            "deleted_at" => delete.deleted_at = time(&value)?,
            _ => return None,
        }
    }
    (!delete.id.is_empty()).then_some(delete)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Box<RawValue> {
        serde_json::from_str(text).expect("raw value")
    }

    #[test]
    fn a_full_entry_is_read_field_by_field() {
        let entry = raw(
            r#"{"id":"a","content":"c","scope":"global","metadata":{"k":"v","k":"w"},
            "embedding":[],"embedding_source":"s","created_at":"2026-10-26T01:00:00Z",
            "updated_at":"2026-10-26T01:00:01.5Z","importance":0.25,"valid_from":"2026-10-26T01:00:00Z",
            "valid_to":null,"access_count":7,"decay_factor":0.5,"unknown":{"x":[1]},"entities":["e"],"evidence":null}"#,
        );
        let memory = memory(&entry).expect("claimed");
        assert_eq!(memory.id, "a");
        assert_eq!(memory.metadata.unwrap()["k"], "w");
        assert_eq!(memory.embedding, Some(Vec::new()));
        assert_eq!(memory.access_count, 7);
        assert!(memory.valid_from.is_some() && memory.valid_to.is_none());
        assert!((memory.importance - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn unproven_entries_stay_on_go() {
        for text in [
            r"{}",
            r#"{"id":""}"#,
            r#"{"id":null}"#,
            r#"{"id":5}"#,
            r#"{"ID":"a"}"#,
            r#"{"id":"a","id":"b"}"#,
            r#"{"id":"a","metadata":{"k":5}}"#,
            r#"{"id":"a","metadata":{"k":null}}"#,
            r#"{"id":"a","metadata":[]}"#,
            r#"{"id":"a","embedding":[1]}"#,
            r#"{"id":"a","importance":"1"}"#,
            r#"{"id":"a","importance":1e999}"#,
            r#"{"id":"a","access_count":1.5}"#,
            r#"{"id":"a","access_count":1e2}"#,
            r#"{"id":"a","access_count":9223372036854775808}"#,
            r#"{"id":"a","updated_at":"2026-10-26T03:00:00+02:00"}"#,
            r#"{"id":"a","updated_at":5}"#,
            r#"{"id":"a","entities":[1]}"#,
            r#"{"id":"a","evidence":[{}]}"#,
            r#"{"id":"a","valid_from":"bad"}"#,
            r"[]",
            r#""a""#,
        ] {
            assert!(memory(&raw(text)).is_none(), "claimed: {text}");
        }
    }

    #[test]
    fn tombstones_need_a_string_id_and_a_canonical_time() {
        let delete = tombstone(&raw(
            r#"{"id":"a","deleted_at":"2026-10-26T01:00:00Z","x":1}"#,
        ))
        .expect("claimed");
        assert_eq!(delete.id, "a");
        let absent = tombstone(&raw(r#"{"id":"a"}"#)).expect("claimed");
        assert_eq!(absent.deleted_at, symbrain_memory::zero_time());
        for text in [
            r#"{"id":"a","deleted_at":"x"}"#,
            r#"{"id":1}"#,
            r#"{"deleted_at":"2026-10-26T01:00:00Z"}"#,
            r#"{"ID":"a"}"#,
            r"null",
        ] {
            assert!(tombstone(&raw(text)).is_none(), "claimed: {text}");
        }
    }
}
