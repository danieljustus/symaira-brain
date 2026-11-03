//! Request-body decoding of `/api/sync/apply` and `/api/sync/relay`.
//!
//! `encoding/json` semantics that matter on this wire: object keys match the
//! struct tags case-insensitively and are applied in document order (the last
//! one wins), `null` leaves a scalar untouched but clears a map or slice, and
//! any type mismatch fails the whole body, which the handler reports as
//! `400 Bad request body`.

use crate::{PulledMemory, zero_time};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;

/// Go's `foldName` for the ASCII field names: the long s and the Kelvin sign
/// fold onto `s` and `k`.
fn fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{17f}' => 's',
            '\u{212a}' => 'k',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// One decoded request: memories first, then tombstones, in document order.
#[derive(Default)]
pub(super) struct Apply {
    pub memories: Vec<PulledMemory>,
    pub deleted: Vec<(String, DateTime<Utc>)>,
}

/// Decodes a top-level object (or `null`) into its members; duplicate keys
/// collapse with the last one winning, as in `encoding/json`.
pub(super) fn top_level(body: &[u8]) -> Option<Vec<(String, Value)>> {
    match serde_json::from_slice::<Value>(body).ok()? {
        Value::Null => Some(Vec::new()),
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

pub(super) fn apply(body: &[u8]) -> Option<Apply> {
    let pairs = top_level(body)?;
    let mut out = Apply::default();
    for (key, value) in pairs {
        match (fold(&key).as_str(), &value) {
            ("memories", Value::Null) => out.memories.clear(),
            ("memories", Value::Array(items)) => {
                out.memories.clear();
                for item in items {
                    out.memories.push(memory(item)?);
                }
            }
            ("deleted", Value::Null) => out.deleted.clear(),
            ("deleted", Value::Array(items)) => {
                out.deleted.clear();
                for item in items {
                    out.deleted.push(tombstone(item)?);
                }
            }
            ("memories" | "deleted", _) => return None,
            _ => {}
        }
    }
    Some(out)
}

fn text(value: &Value, target: &mut String) -> Option<()> {
    match value {
        Value::Null => Some(()),
        Value::String(text) => {
            text.clone_into(target);
            Some(())
        }
        _ => None,
    }
}

fn instant(value: &Value, target: &mut DateTime<Utc>) -> Option<()> {
    match value {
        Value::Null => Some(()),
        Value::String(text) => {
            *target = super::rfc3339::parse(text)?;
            Some(())
        }
        _ => None,
    }
}

fn optional_instant(value: &Value, target: &mut Option<DateTime<Utc>>) -> Option<()> {
    match value {
        Value::Null => {
            *target = None;
            Some(())
        }
        Value::String(text) => {
            *target = Some(super::rfc3339::parse(text)?);
            Some(())
        }
        _ => None,
    }
}

fn float(value: &Value, target: &mut f64) -> Option<()> {
    match value {
        Value::Null => Some(()),
        Value::Number(number) => {
            *target = number.as_f64().filter(|v| v.is_finite())?;
            Some(())
        }
        _ => None,
    }
}

fn memory(value: &Value) -> Option<PulledMemory> {
    // A JSON null element is a nil pointer in Go and panics the handler;
    // anything else is a type error. Both are refused here.
    let Value::Object(map) = value else {
        return None;
    };
    let mut memory = PulledMemory::zero();
    for (key, value) in map {
        let key = fold(key);
        match key.as_str() {
            "id" => text(&value, &mut memory.id)?,
            "content" => text(&value, &mut memory.content)?,
            "scope" => text(&value, &mut memory.scope)?,
            "metadata" => match &value {
                Value::Null => memory.metadata = None,
                Value::Object(map) => {
                    let mut out = BTreeMap::new();
                    for (name, entry) in map {
                        out.insert(
                            name.clone(),
                            match entry {
                                Value::String(text) => text.clone(),
                                Value::Null => String::new(),
                                _ => return None,
                            },
                        );
                    }
                    memory.metadata = Some(out);
                }
                _ => return None,
            },
            "embedding" => match &value {
                Value::Null => memory.embedding = None,
                Value::Array(items) => {
                    let mut vector = Vec::with_capacity(items.len());
                    for item in items {
                        vector.push(match item {
                            Value::Null => 0.0,
                            Value::Number(number) => {
                                #[allow(clippy::cast_possible_truncation)]
                                let narrowed = number.as_f64()? as f32;
                                if !narrowed.is_finite() {
                                    return None;
                                }
                                narrowed
                            }
                            _ => return None,
                        });
                    }
                    memory.embedding = Some(vector);
                }
                _ => return None,
            },
            "embedding_source" => text(&value, &mut memory.embedding_source)?,
            "embedding_model" => text(&value, &mut memory.embedding_model)?,
            "embedding_quantization" => text(&value, &mut memory.embedding_quantization)?,
            "content_hash" => text(&value, &mut memory.content_hash)?,
            "created_at" => instant(&value, &mut memory.created_at)?,
            "updated_at" => instant(&value, &mut memory.updated_at)?,
            "created_by" => text(&value, &mut memory.created_by)?,
            "updated_by" => text(&value, &mut memory.updated_by)?,
            "created_session" => text(&value, &mut memory.created_session)?,
            "updated_session" => text(&value, &mut memory.updated_session)?,
            "entities" => match &value {
                Value::Null => {}
                Value::Array(items) => {
                    if !items.iter().all(|i| matches!(i, Value::String(_) | Value::Null)) {
                        return None;
                    }
                }
                _ => return None,
            },
            "consolidation_status" => text(&value, &mut memory.consolidation_status)?,
            "consolidated_into_id" => text(&value, &mut memory.consolidated_into_id)?,
            "importance" => float(&value, &mut memory.importance)?,
            "valid_from" => optional_instant(&value, &mut memory.valid_from)?,
            "valid_to" => optional_instant(&value, &mut memory.valid_to)?,
            "superseded_by" => text(&value, &mut memory.superseded_by)?,
            "kind" => text(&value, &mut memory.kind)?,
            "review_status" => text(&value, &mut memory.review_status)?,
            "decay_factor" => float(&value, &mut memory.decay_factor)?,
            "retired_at" => optional_instant(&value, &mut memory.retired_at)?,
            "tier" => text(&value, &mut memory.tier)?,
            "expires_at" => optional_instant(&value, &mut memory.expires_at)?,
            "access_count" => match &value {
                Value::Null => {}
                Value::Number(number) => memory.access_count = number.as_i64()?,
                _ => return None,
            },
            "last_access" => optional_instant(&value, &mut memory.last_access)?,
            "prev_access" => optional_instant(&value, &mut memory.prev_access)?,
            "evidence" => match &value {
                Value::Null => {}
                Value::Array(items) => {
                    if !items.iter().all(|i| matches!(i, Value::Object(_) | Value::Null)) {
                        return None;
                    }
                }
                _ => return None,
            },
            _ => {}
        }
    }
    Some(memory)
}

fn tombstone(value: &Value) -> Option<(String, DateTime<Utc>)> {
    let mut id = String::new();
    let mut at = zero_time();
    match value {
        Value::Null => {}
        Value::Object(map) => {
            for (key, value) in map {
                match fold(key).as_str() {
                    "id" => text(value, &mut id)?,
                    "deleted_at" => instant(value, &mut at)?,
                    _ => {}
                }
            }
        }
        _ => return None,
    }
    Some((id, at))
}

/// One relay request blob.
pub(super) struct Blob {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub data: Vec<u8>,
}

pub(super) fn relay(body: &[u8]) -> Option<Vec<Blob>> {
    let pairs = top_level(body)?;
    let mut blobs = Vec::new();
    for (key, value) in pairs {
        if fold(&key) != "blobs" {
            continue;
        }
        blobs.clear();
        match &value {
            Value::Null => {}
            Value::Array(items) => {
                for item in items {
                    let mut blob = Blob {
                        id: String::new(),
                        updated_at: zero_time(),
                        data: Vec::new(),
                    };
                    match item {
                        Value::Null => {}
                        Value::Object(map) => {
                            for (name, value) in map {
                                match fold(name).as_str() {
                                    "id" => text(value, &mut blob.id)?,
                                    "updated_at" => instant(value, &mut blob.updated_at)?,
                                    "blob" => match value {
                                        Value::Null => blob.data.clear(),
                                        Value::String(encoded) => {
                                            blob.data = STANDARD.decode(encoded).ok()?;
                                        }
                                        _ => return None,
                                    },
                                    _ => {}
                                }
                            }
                        }
                        _ => return None,
                    }
                    blobs.push(blob);
                }
            }
            _ => return None,
        }
    }
    Some(blobs)
}

/// `uuid.Parse`: canonical, braced, `urn:uuid:` and plain 32-digit forms.
pub(super) fn uuid_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    let canonical = |b: &[u8]| {
        b.len() >= 36
            && b[..36].iter().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
    };
    match bytes.len() {
        36 => canonical(bytes),
        45 => bytes[..9].eq_ignore_ascii_case(b"urn:uuid:") && canonical(&bytes[9..]),
        38 => canonical(&bytes[1..]),
        32 => bytes.iter().all(u8::is_ascii_hexdigit),
        _ => false,
    }
}

/// Arguments of `POST /api/set`.
#[derive(Default)]
pub(super) struct SetArgs {
    pub content: String,
    pub scope: String,
    pub metadata: BTreeMap<String, String>,
    pub session_id: String,
    pub entities: Vec<String>,
    pub working: bool,
}

fn flag(value: &Value, target: &mut bool) -> Option<()> {
    match value {
        Value::Null => Some(()),
        Value::Bool(flag) => {
            *target = *flag;
            Some(())
        }
        _ => None,
    }
}

pub(super) fn set_args(body: &[u8]) -> Option<SetArgs> {
    let mut args = SetArgs::default();
    for (key, value) in top_level(body)? {
        match fold(&key).as_str() {
            "content" => text(&value, &mut args.content)?,
            "scope" => text(&value, &mut args.scope)?,
            "session_id" => text(&value, &mut args.session_id)?,
            "working" => flag(&value, &mut args.working)?,
            "metadata" => match &value {
                Value::Null => args.metadata.clear(),
                Value::Object(map) => {
                    args.metadata.clear();
                    for (name, entry) in map {
                        args.metadata.insert(
                            name.clone(),
                            match entry {
                                Value::String(text) => text.clone(),
                                Value::Null => String::new(),
                                _ => return None,
                            },
                        );
                    }
                }
                _ => return None,
            },
            "entities" => match &value {
                Value::Null => args.entities.clear(),
                Value::Array(items) => {
                    args.entities.clear();
                    for item in items {
                        args.entities.push(match item {
                            Value::String(text) => text.clone(),
                            Value::Null => String::new(),
                            _ => return None,
                        });
                    }
                }
                _ => return None,
            },
            _ => {}
        }
    }
    Some(args)
}

/// Arguments of `POST /api/search`; `extended` is set when a filter beyond
/// query/scope/limit is in use.
#[derive(Default)]
pub(super) struct SearchArgs {
    pub query: String,
    pub scope: String,
    pub limit: i64,
    pub extended: bool,
}

pub(super) fn search_args(body: &[u8]) -> Option<SearchArgs> {
    let mut args = SearchArgs::default();
    for (key, value) in top_level(body)? {
        match fold(&key).as_str() {
            "query" => text(&value, &mut args.query)?,
            "scope" => text(&value, &mut args.scope)?,
            "limit" => match &value {
                Value::Null => {}
                Value::Number(number) => args.limit = number.as_i64()?,
                _ => return None,
            },
            "exclude_superseded" => {
                let mut on = false;
                flag(&value, &mut on)?;
                args.extended |= on;
            }
            "entity" | "min_confidence" | "verification" | "max_age" | "max_sensitivity"
            | "min_sharing_level" | "client_id" => {
                let mut set = String::new();
                text(&value, &mut set)?;
                args.extended |= !set.is_empty();
            }
            _ => {}
        }
    }
    Some(args)
}
