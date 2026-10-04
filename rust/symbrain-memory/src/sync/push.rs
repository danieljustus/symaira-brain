//! Bounded independent memory/delete keysets; a failed batch keeps old cursor.

use super::runner::max_time;
use super::{
    Options, RelayBlob, RelayCodec, RelayPayload, RunContext, SyncError, SyncResult, SyncTime,
    SyncTransport,
};

const BATCH: i64 = 200;

pub(super) fn push(
    context: &RunContext,
    transport: &mut impl SyncTransport,
    mut codec: Option<&mut dyn RelayCodec>,
    options: &Options<'_>,
    since: SyncTime,
    result: &mut SyncResult,
) -> Result<SyncTime, SyncError> {
    let store = options
        .store
        .ok_or_else(|| SyncError("database is required".into()))?;
    let mut memory_since = since;
    let mut deleted_since = since;
    let mut memory_id = String::new();
    let mut deleted_id = String::new();
    let mut max_sent = SyncTime::default();
    loop {
        let memories = store
            .sync_memories_since(memory_since, &memory_id, BATCH)
            .map_err(|error| SyncError(format!("read local changes: {error}")))?;
        let deleted = store
            .sync_deleted_since(deleted_since, &deleted_id, BATCH)
            .map_err(|error| SyncError(format!("read local tombstones: {error}")))?;
        if memories.is_empty() && deleted.is_empty() {
            return Ok(max_sent);
        }
        if let Some(codec) = codec.as_mut() {
            let mut blobs = Vec::with_capacity(memories.len() + deleted.len());
            for memory in &memories {
                let payload = RelayPayload {
                    memory: Some(memory.clone()),
                    deleted: None,
                };
                blobs.push(RelayBlob {
                    id: memory.id.clone(),
                    updated_at: memory.updated_at,
                    blob: encrypt(*codec, &payload, options.passphrase)?,
                });
            }
            for item in &deleted {
                let payload = RelayPayload {
                    memory: None,
                    deleted: Some(item.clone()),
                };
                blobs.push(RelayBlob {
                    id: format!("tombstone:{}", item.id),
                    updated_at: item.deleted_at,
                    blob: encrypt(*codec, &payload, options.passphrase)?,
                });
            }
            result.relay_blobs_stored += transport.relay_push(context, &blobs)?.stored;
        } else {
            let applied = transport.apply(context, &memories, &deleted)?;
            result.pushed_memories += applied.applied;
            result.pushed_deletes += applied.deleted;
        }
        for memory in &memories {
            max_sent = max_time(max_sent, memory.updated_at);
        }
        for item in &deleted {
            max_sent = max_time(max_sent, item.deleted_at);
        }
        if let Some(last) = memories.last() {
            memory_since = last.updated_at;
            memory_id.clone_from(&last.id);
        }
        if let Some(last) = deleted.last() {
            deleted_since = last.deleted_at;
            deleted_id.clone_from(&last.id);
        }
        if memories.len() < 200 && deleted.len() < 200 {
            return Ok(max_sent);
        }
    }
}

fn encrypt(
    codec: &mut dyn RelayCodec,
    payload: &RelayPayload,
    passphrase: &str,
) -> Result<Vec<u8>, SyncError> {
    let plaintext = codec
        .encode(payload)
        .map_err(|error| SyncError(format!("encode relay payload: {error}")))?;
    codec
        .encrypt(&plaintext, passphrase)
        .map_err(|error| SyncError(format!("encrypt relay payload: {error}")))
}
