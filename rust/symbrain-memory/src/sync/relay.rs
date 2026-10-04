//! Relay orchestration shares the existing store; no cryptography is invented.

use super::runner::max_time;
use super::{Options, RelayPayload, RunContext, SyncError, SyncResult, SyncTime, SyncTransport};

/// A run-owned interoperable crypto/JSON codec, never a credential store.
///
/// Before admission the concrete implementation must prove Go's v1 and legacy
/// AES-256-GCM payloads, PBKDF2-SHA256/600000, authenticated rejection, legacy
/// fallback and salt/nonce lifetime. No concrete implementation is admitted by
/// this source-stage interface. It must never log or persist the passphrase.
pub trait RelayCodec {
    /// Discards phase-local key/salt cache without drawing entropy. Frozen Go
    /// constructs separate engines for pull and push, lazily deriving keys.
    fn reset_phase(&mut self);
    fn encode(&self, payload: &RelayPayload) -> Result<Vec<u8>, SyncError>;
    fn decode(&self, payload: &[u8]) -> Result<RelayPayload, SyncError>;
    fn encrypt(&mut self, payload: &[u8], passphrase: &str) -> Result<Vec<u8>, SyncError>;
    fn decrypt(&mut self, blob: &[u8], passphrase: &str) -> Result<Vec<u8>, SyncError>;
}

pub(super) fn pull(
    context: &RunContext,
    transport: &mut impl SyncTransport,
    codec: &mut dyn RelayCodec,
    options: &Options<'_>,
    mut since: SyncTime,
    limit: i64,
    result: &mut SyncResult,
) -> Result<SyncTime, SyncError> {
    let store = options
        .store
        .ok_or_else(|| SyncError("database is required".into()))?;
    let mut max_seen = SyncTime::default();
    loop {
        let response = transport.relay_pull(context, since, limit)?;
        max_seen = max_time(max_seen, response.server_time);
        for blob in &response.blobs {
            let plaintext = codec
                .decrypt(&blob.blob, options.passphrase)
                .map_err(|error| SyncError(format!("decrypt relay blob {}: {error}", blob.id)))?;
            let payload = codec
                .decode(&plaintext)
                .map_err(|error| SyncError(format!("decode relay blob {}: {error}", blob.id)))?;
            // Both fields can be supplied by a peer: deletion takes precedence.
            // Go does not bind the outer blob ID/clock to the decrypted payload.
            if let Some(deleted) = payload.deleted {
                result.pulled_deletes_applied +=
                    i64::from(store.apply_sync_delete(&deleted).map_err(|error| {
                        SyncError(format!("apply relay delete {}: {error}", deleted.id))
                    })?);
                result.relay_blobs_fetched += 1;
            } else if let Some(memory) = payload.memory {
                result.pulled_memories_applied += i64::from(
                    store
                        .upsert_sync_memory(&memory, options.quantize_binary)
                        .map_err(|error| {
                            SyncError(format!("apply relay memory {}: {error}", memory.id))
                        })?,
                );
                result.relay_blobs_fetched += 1;
            }
            max_seen = max_time(max_seen, blob.updated_at);
        }
        if i64::try_from(response.blobs.len()).unwrap_or(i64::MAX) < limit {
            return Ok(max_seen);
        }
        // Frozen protocol uses timestamp-only relay pagination, not an ID cursor.
        if let Some(last) = response.blobs.last() {
            since = last.updated_at;
        }
    }
}
