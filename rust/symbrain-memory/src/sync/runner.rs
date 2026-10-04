//! Single-run ordering, clocks and failure boundaries from the frozen runner.

use super::{
    ApplyResult, Changes, DeletedMemory, RelayBlob, RelayChanges, RelayCodec, RelayPushResult,
    SyncMemory, SyncTime,
};
use crate::{Store, StoreError};
use serde::Serialize;
use std::time::{Duration, Instant};

/// Sync failures are returned; no diagnostics are emitted by the store owner.
#[derive(Debug)]
pub struct SyncError(pub String);
impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for SyncError {}
impl From<StoreError> for SyncError {
    fn from(error: StoreError) -> Self {
        Self(error.to_string())
    }
}

/// Request transports must bound connection, response and decoding by this
/// deadline, in addition to their own sixty-second HTTP client timeout.
pub struct RunContext {
    deadline: Instant,
}
impl RunContext {
    /// Preserves an earlier caller deadline (including an already expired one).
    #[must_use]
    pub fn with_deadline(deadline: Instant) -> Self {
        Self { deadline }
    }
    /// # Errors
    /// Returns the context deadline error if the run has expired.
    pub fn remaining(&self) -> Result<Duration, SyncError> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| SyncError("context deadline exceeded".into()))
    }
}

/// Transport-owned Go URL policy, bearer headers, bounded response decoding
/// and redirects. Implementing this trait alone does not admit CLI fallback.
pub trait SyncTransport {
    /// Called before the first cursor read or request.
    fn validate(&self, remote: &str, allow_insecure_http: bool) -> Result<(), SyncError>;
    fn changes(
        &mut self,
        context: &RunContext,
        since: SyncTime,
        cursor: &str,
        limit: i64,
    ) -> Result<Changes, SyncError>;
    fn apply(
        &mut self,
        context: &RunContext,
        memories: &[SyncMemory],
        deleted: &[DeletedMemory],
    ) -> Result<ApplyResult, SyncError>;
    fn relay_pull(
        &mut self,
        context: &RunContext,
        since: SyncTime,
        limit: i64,
    ) -> Result<RelayChanges, SyncError>;
    fn relay_push(
        &mut self,
        context: &RunContext,
        blobs: &[RelayBlob],
    ) -> Result<RelayPushResult, SyncError>;
}

/// One run against the existing store. Neither bearer tokens nor relay
/// passphrases are persisted. Debug is intentionally absent for secrets.
pub struct Options<'a> {
    pub remote: &'a str,
    pub store: Option<&'a Store>,
    pub pull: bool,
    pub push: bool,
    pub encrypted_relay: bool,
    pub passphrase: &'a str,
    pub allow_insecure_http: bool,
    pub page_limit: i64,
    pub timeout: Duration,
    pub quantize_binary: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncResult {
    pub remote: String,
    pub mode: String,
    pub encrypted_relay: bool,
    pub cursor: SyncTime,
    pub server_time: SyncTime,
    pub pulled_memories_applied: i64,
    pub pulled_deletes_applied: i64,
    pub pushed_memories: i64,
    pub pushed_deletes: i64,
    pub relay_blobs_fetched: i64,
    pub relay_blobs_stored: i64,
}

/// Executes one pull/push sequence; no automatic retry or rollback is added.
/// # Errors
/// Returns validation, cursor, transport, crypto or mutation errors. Earlier
/// applied rows survive failures, while the prior cursor stays unchanged.
pub fn run(
    options: &Options<'_>,
    transport: &mut impl SyncTransport,
    mut codec: Option<&mut dyn RelayCodec>,
    caller: Option<&RunContext>,
) -> Result<SyncResult, SyncError> {
    if options.remote.is_empty() {
        return Err(SyncError("remote URL is required".into()));
    }
    let store = options
        .store
        .ok_or_else(|| SyncError("database is required".into()))?;
    if !options.pull && !options.push {
        return Err(SyncError(
            "at least one of pull or push must be enabled".into(),
        ));
    }
    if options.encrypted_relay && options.passphrase.is_empty() {
        return Err(SyncError("encrypted relay requires a passphrase".into()));
    }
    transport.validate(options.remote, options.allow_insecure_http)?;
    let timeout = if options.timeout.is_zero() {
        Duration::from_secs(60)
    } else {
        options.timeout
    };
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| SyncError("sync timeout exceeds the clock range".into()))?;
    let context =
        RunContext::with_deadline(caller.map_or(deadline, |caller| caller.deadline.min(deadline)));
    let cursor = store
        .sync_cursor(options.remote)
        .map_err(|error| SyncError(format!("read sync cursor for {}: {error}", options.remote)))?;
    let mut result = SyncResult {
        remote: options.remote.into(),
        mode: if options.pull && options.push {
            "both"
        } else if options.pull {
            "pull"
        } else {
            "push"
        }
        .into(),
        encrypted_relay: options.encrypted_relay,
        ..SyncResult::default()
    };
    let limit = if options.page_limit <= 0 {
        500
    } else {
        options.page_limit
    };
    let mut server_time = SyncTime::default();
    let mut local_max = SyncTime::default();
    if options.encrypted_relay {
        let codec = codec
            .as_deref_mut()
            .ok_or_else(|| SyncError("native relay codec is not configured".into()))?;
        codec.reset_phase();
        // Frozen Go pulls in relay mode even for --push without --pull.
        server_time = super::relay::pull(
            &context,
            transport,
            codec,
            options,
            cursor,
            limit,
            &mut result,
        )
        .map_err(|error| SyncError(format!("relay pull from {}: {error}", options.remote)))?;
        if options.push {
            codec.reset_phase();
            local_max = super::push::push(
                &context,
                transport,
                Some(codec),
                options,
                cursor,
                &mut result,
            )
            .map_err(|error| SyncError(format!("relay push to {}: {error}", options.remote)))?;
        }
    } else {
        if options.pull {
            server_time = plain_pull(&context, transport, options, cursor, limit, &mut result)
                .map_err(|error| SyncError(format!("pull from {}: {error}", options.remote)))?;
        }
        if options.push {
            local_max = super::push::push(&context, transport, None, options, cursor, &mut result)
                .map_err(|error| SyncError(format!("push to {}: {error}", options.remote)))?;
        }
    }
    result.server_time = server_time;
    // Including the old cursor here would fix an inherited regression/reset,
    // but would change the frozen protocol. That decision is tracked separately.
    result.cursor = max_time(server_time, local_max);
    store
        .set_sync_cursor(options.remote, result.cursor)
        .map_err(|error| {
            SyncError(format!(
                "persist sync cursor for {}: {error}",
                options.remote
            ))
        })?;
    Ok(result)
}

fn plain_pull(
    context: &RunContext,
    transport: &mut impl SyncTransport,
    options: &Options<'_>,
    mut since: SyncTime,
    limit: i64,
    result: &mut SyncResult,
) -> Result<SyncTime, SyncError> {
    let store = options
        .store
        .ok_or_else(|| SyncError("database is required".into()))?;
    let mut max_seen = SyncTime::default();
    let mut cursor = String::new();
    loop {
        let changes = transport.changes(context, since, &cursor, limit)?;
        for memory in changes.memories.unwrap_or_default() {
            let applied = store
                .upsert_sync_memory(&memory, options.quantize_binary)
                .map_err(|error| {
                    SyncError(format!("apply remote memory {}: {error}", memory.id))
                })?;
            result.pulled_memories_applied += i64::from(applied);
            max_seen = max_time(max_seen, memory.updated_at);
        }
        for deleted in changes.deleted.unwrap_or_default() {
            let removed = store.apply_sync_delete(&deleted).map_err(|error| {
                SyncError(format!("apply remote delete {}: {error}", deleted.id))
            })?;
            result.pulled_deletes_applied += i64::from(removed);
            max_seen = max_time(max_seen, deleted.deleted_at);
        }
        max_seen = max_time(max_seen, changes.server_time);
        if changes.next_cursor.is_empty() {
            return Ok(max_seen);
        }
        cursor = changes.next_cursor;
        since = SyncTime::default();
    }
}

pub(super) fn max_time(left: SyncTime, right: SyncTime) -> SyncTime {
    if left > right { left } else { right }
}
