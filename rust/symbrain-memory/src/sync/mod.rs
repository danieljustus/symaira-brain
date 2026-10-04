//! Memory synchronization on the existing SQLite owner.
//!
//! This source-stage API is deliberately not wired into CLI or HTTP admission.
//! Transport validation/decoding and relay cryptography are supplied by their
//! owners; no secret, HTTP listener or second database is created here.

mod model;
mod push;
mod relay;
mod runner;
mod store;
mod time;
mod upsert;

pub use model::{
    ApplyResult, Changes, DeletedMemory, RelayBlob, RelayChanges, RelayPayload, RelayPushResult,
    SyncEvidence, SyncMemory,
};
pub use relay::RelayCodec;
pub use runner::{Options, RunContext, SyncError, SyncResult, SyncTransport, run};
pub use time::SyncTime;

#[cfg(test)]
mod tests;
