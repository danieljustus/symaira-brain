//! Audit hash chain and checkpoint anchor.
//!
//! SEC-002 port of the `guard/internal/audit` chain API, which itself
//! delegates to corekit/auditkit: SHA-256 entry digests mixed with the
//! previous hash, chain verification, the external anchor that detects
//! tail truncation, and anchor decoding. The checkpoint WRITE path is not
//! ported (nothing in Rust writes anchors yet); error bytes for
//! malformed anchor JSON are encoding/json-specific and unpinned.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// Genesis previous-hash value for the first entry (`auditkit.GenesisHash`).
pub const GENESIS_HASH: &str = "";

/// Current `ChainAnchor` schema version (`auditkit.AnchorSchemaVersion`).
pub const ANCHOR_SCHEMA_VERSION: i32 = 2;

/// External checkpoint that detects audit-log truncation
/// (`auditkit.ChainAnchor`). Field order and `omitempty` semantics mirror
/// the Go struct tags.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChainAnchor {
    /// SHA-256 of the most recent audit entry line.
    #[serde(default)]
    pub last_entry_hash: String,
    /// Total number of non-empty entries in the log.
    #[serde(default)]
    pub entry_count: i64,
    /// Anchor schema version.
    #[serde(default)]
    pub schema_version: i32,
    /// Exact byte size of the checkpointed log file (`json:"log_size,omitempty"`).
    #[serde(default, skip_serializing_if = "zero_i64")]
    pub log_size: i64,
    /// SHA-256 of the complete checkpointed log bytes
    /// (`json:"content_hash,omitempty"`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes &field.
fn zero_i64(value: &i64) -> bool {
    *value == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `auditkit.HashEntry`: SHA-256 over `prev_hash + entry_data`, so
/// modifying any retained entry breaks every subsequent link.
#[must_use]
pub fn hash_entry(entry_data: &str, prev_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(entry_data.as_bytes());
    hex_encode(&hasher.finalize())
}

/// `auditkit.VerifyChain`: walks entries from `initial_hash` (empty
/// entries skipped, so trailing newlines cannot break verification) and
/// compares the final hash.
#[must_use]
pub fn verify_chain(entries: &[String], initial_hash: &str, expected_final_hash: &str) -> bool {
    let mut prev = prev_start(initial_hash);
    for entry in entries {
        if entry.is_empty() {
            continue;
        }
        prev = hash_entry(entry, &prev);
    }
    prev == expected_final_hash
}

fn prev_start(initial_hash: &str) -> String {
    initial_hash.to_owned()
}

/// `auditkit.VerifyAnchor`: entry count must match the checkpoint
/// (truncation detection) before the chain is checked (tamper
/// detection). A missing anchor fails closed.
#[must_use]
pub fn verify_anchor(entries: &[String], initial_hash: &str, anchor: Option<&ChainAnchor>) -> bool {
    let Some(anchor) = anchor else {
        return false;
    };
    let non_empty = entries.iter().filter(|entry| !entry.is_empty()).count();
    match i64::try_from(non_empty) {
        Ok(count) if count == anchor.entry_count => {}
        _ => return false,
    }
    verify_chain(entries, initial_hash, &anchor.last_entry_hash)
}

/// `auditkit.VerifyAnchorForLog`: additionally requires the anchor to
/// authenticate the complete log bytes (size + content hash), so a
/// same-size modified log is rejected and legacy anchors without a
/// content hash fail closed.
#[must_use]
pub fn verify_anchor_for_log(
    entries: &[String],
    initial_hash: &str,
    anchor: Option<&ChainAnchor>,
    log_size: i64,
    content_hash: &str,
) -> bool {
    let Some(anchor) = anchor else {
        return false;
    };
    if anchor.log_size != log_size
        || anchor.content_hash.is_empty()
        || anchor.content_hash != content_hash
    {
        return false;
    }
    verify_anchor(entries, initial_hash, Some(anchor))
}

/// `auditkit.ReadCheckpoint` without the file layer: callers pass the
/// anchor bytes, or `None` for a missing anchor file (Go returns
/// `(nil, nil)` there, which callers treat as "unverified").
///
/// # Errors
/// Returns the Go-prefixed diagnostic; the inner text is `serde_json`'s
/// wording, which the oracle does not pin (`encoding/json` wording has no
/// `serde` equivalent).
pub fn parse_anchor(data: &[u8]) -> Result<ChainAnchor, String> {
    serde_json::from_slice(data).map_err(|error| format!("auditkit: parse anchor: {error}"))
}
