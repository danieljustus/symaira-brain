//! Redacting, tamper-evident JSONL audit logging for Symaira Brain.

#![deny(unsafe_code)]

mod logger;
mod model;
mod redact;
mod sink;
mod tail;

pub use logger::Logger;
pub use model::{Classification, Config, Degradation, Entry, Exposure};
pub use redact::redact_args;
pub use sink::{Sink, hash_entry};
pub use tail::{audit_log_paths, latest_degradations_in, tail_entries_bounded, tail_entries_in};

use std::io;

/// Reads entries from the process XDG audit directory.
///
/// # Errors
/// Returns an error when the audit directory cannot be resolved or discovered.
pub fn tail_entries(profile: &str, limit: usize) -> io::Result<Vec<Entry>> {
    let dir = symbrain_core::xdg::audit_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "audit: resolve audit dir: home directory unavailable",
        )
    })?;
    tail_entries_in(&dir, profile, limit)
}

/// Reads latest-session degradation records from the process XDG audit directory.
///
/// # Errors
/// Returns an error when the audit directory cannot be resolved or read.
pub fn latest_degradations(profile: &str) -> io::Result<Vec<Degradation>> {
    let dir = symbrain_core::xdg::audit_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "audit: resolve audit dir: home directory unavailable",
        )
    })?;
    latest_degradations_in(&dir, profile)
}
