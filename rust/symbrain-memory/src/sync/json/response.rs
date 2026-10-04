//! Response shapes and custom counter aliases, without losing input order.

use super::super::{
    ApplyResult, Changes, DeletedMemory, RelayChanges, RelayPayload, RelayPushResult, SyncError,
};
use super::{
    memory::{Memory, deleted},
    raw::first,
    strings::field,
    typed::{Decode, Slice, object},
};
use std::collections::BTreeMap;

pub(super) fn changes(bytes: &[u8]) -> Result<Changes, SyncError> {
    let raw = first(bytes, true)?;
    let mut decode = Decode::default();
    let mut result = Changes::default();
    let mut memories: Slice<Memory> = Slice::default();
    let mut deletes: Slice<DeletedMemory> = Slice::default();
    if let Some(members) = object(raw, &mut decode, "syncclient.ChangesResponse", "")? {
        for (name, raw) in members {
            match field(&name).as_str() {
                "memories" => {
                    memories.assign(
                        raw,
                        &mut decode,
                        "[]*db.Memory",
                        "ChangesResponse.memories",
                        |raw, row, decode| {
                            if raw.null() {
                                return Err(SyncError(
                                    "native sync refuses Go nil-memory-pointer panic family".into(),
                                ));
                            }
                            row.assign(raw, decode, "ChangesResponse.memories")
                        },
                    )?;
                    result.memories = memories
                        .visible()
                        .map(|rows| rows.into_iter().map(|row| row.value).collect());
                }
                "deleted" => {
                    deletes.assign(
                        raw,
                        &mut decode,
                        "[]db.DeletedMemory",
                        "ChangesResponse.deleted",
                        |raw, value, decode| deleted(raw, value, decode, "ChangesResponse.deleted"),
                    )?;
                    result.deleted = deletes.visible();
                }
                "server_time" => decode.clock(raw, &mut result.server_time)?,
                "next_cursor" => {
                    decode.string(raw, &mut result.next_cursor, "ChangesResponse.next_cursor")?
                }
                _ => {}
            }
        }
    }
    decode.finish(result)
}
pub(super) fn apply_result(bytes: &[u8]) -> Result<ApplyResult, SyncError> {
    let raw = first(bytes, true)?;
    let mut decode = Decode::default();
    let mut values = BTreeMap::new();
    if let Some(members) = object(raw, &mut decode, "map[string]json.RawMessage", "")? {
        for (name, raw) in members {
            values.insert(name, raw);
        }
    }
    let get = |snake: &str, camel: &str| {
        let mut value = 0;
        let mut ignored = Decode::default();
        if let Some(raw) = values.get(snake).or_else(|| values.get(camel)) {
            ignored.integer(*raw, &mut value, "int", "");
        }
        value
    };
    decode.finish(ApplyResult {
        applied: get("applied", "applied"),
        skipped: get("skipped", "skipped"),
        deleted: get("deleted", "deleted"),
        skipped_invalid_scope: get("skipped_invalid_scope", "skippedInvalidScope"),
        skipped_invalid_id: get("skipped_invalid_id", "skippedInvalidID"),
    })
}
pub(super) fn relay_result(bytes: &[u8]) -> Result<RelayPushResult, SyncError> {
    let raw = first(bytes, true)?;
    let mut decode = Decode::default();
    let mut result = RelayPushResult::default();
    if let Some(members) = object(raw, &mut decode, "syncclient.RelayPushResult", "")? {
        for (name, raw) in members {
            match field(&name).as_str() {
                "stored" => {
                    decode.integer(raw, &mut result.stored, "int", "RelayPushResult.stored")
                }
                "skipped" => {
                    decode.integer(raw, &mut result.skipped, "int", "RelayPushResult.skipped")
                }
                _ => {}
            }
        }
    }
    decode.finish(result)
}
pub(super) fn relay_changes(bytes: &[u8]) -> Result<RelayChanges, SyncError> {
    let raw = first(bytes, true)?;
    let mut decode = Decode::default();
    let mut result = RelayChanges::default();
    let mut blobs: Slice<super::blob::Blob> = Slice::default();
    if let Some(members) = object(raw, &mut decode, "syncclient.RelayResponse", "")? {
        for (name, raw) in members {
            match field(&name).as_str() {
                "blobs" => {
                    blobs.assign(
                        raw,
                        &mut decode,
                        "[]db.RelayBlob",
                        "RelayResponse.blobs",
                        super::blob::assign,
                    )?;
                    result.blobs = blobs
                        .visible()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|blob| blob.value)
                        .collect();
                }
                "server_time" => decode.clock(raw, &mut result.server_time)?,
                _ => {}
            }
        }
    }
    decode.finish(result)
}
pub(super) fn relay_payload(bytes: &[u8]) -> Result<RelayPayload, SyncError> {
    let raw = first(bytes, false)?;
    let mut decode = Decode::default();
    let mut result = RelayPayload::default();
    let mut memory = Memory::default();
    if let Some(members) = object(raw, &mut decode, "syncclient.relayPayload", "")? {
        for (name, raw) in members {
            match field(&name).as_str() {
                "memory" => {
                    if raw.null() {
                        result.memory = None;
                        memory = Memory::default();
                    } else {
                        memory.assign(raw, &mut decode, "relayPayload.memory")?;
                        result.memory = Some(memory.value.clone());
                    }
                }
                "deleted" => {
                    if raw.null() {
                        result.deleted = None;
                    } else {
                        deleted(
                            raw,
                            result.deleted.get_or_insert_default(),
                            &mut decode,
                            "relayPayload.deleted",
                        )?;
                    }
                }
                _ => {}
            }
        }
    }
    decode.finish(result)
}
pub(super) fn api_error(bytes: &[u8]) -> (String, String) {
    let Ok(raw) = first(bytes, true) else {
        return (String::new(), String::new());
    };
    let mut decode = Decode::default();
    let mut code = String::new();
    let mut message = String::new();
    if let Ok(Some(members)) = object(raw, &mut decode, "struct { Error string; Code string }", "")
    {
        for (name, raw) in members {
            match field(&name).as_str() {
                "code" => {
                    let _ = decode.string(raw, &mut code, "");
                }
                "error" => {
                    let _ = decode.string(raw, &mut message, "");
                }
                _ => {}
            }
        }
    }
    (code, message)
}
