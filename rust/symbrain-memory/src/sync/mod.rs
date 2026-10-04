//! Memory synchronization on the existing SQLite owner.
//!
//! This source-stage API is deliberately not wired into CLI or HTTP admission.
//! Transport validation/decoding and relay cryptography are supplied by their
//! owners; no secret, HTTP listener or second database is created here.

mod crypto;
mod http;
mod json;
mod model;
mod push;
mod relay;
mod remote;
mod runner;
mod store;
mod time;
mod upsert;
mod wire;

pub use crypto::{CryptoEngine, EntropySource};
pub use http::HttpSyncTransport;
pub use model::{
    ApplyResult, Changes, DeletedMemory, RelayBlob, RelayChanges, RelayPayload, RelayPushResult,
    SyncEvidence, SyncMemory,
};
pub use relay::RelayCodec;
pub use remote::validate_remote_url;
pub use runner::{Options, RunContext, SyncError, SyncResult, SyncTransport, run};
pub use time::SyncTime;

#[cfg(test)]
mod backend_tests;
#[cfg(test)]
mod tests;
