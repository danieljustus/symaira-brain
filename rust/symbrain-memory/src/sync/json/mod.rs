//! Bounded ordered JSON decoding following pinned Go1.26.7 decoder contracts.
//!
//! Raw container scanning is iterative, including ignored metadata at depth
//! 10000. Typed fields merge in input order; no Value map discards duplicates.
//! Exact syntax/time/transport diagnostics remain actual-oracle release gates.

mod base64;
mod blob;
mod memory;
mod raw;
mod response;
mod strings;
mod typed;

pub(super) use response::{
    api_error, apply_result, changes, relay_changes, relay_payload, relay_result,
};

pub(super) fn first_range(bytes: &[u8]) -> Result<std::ops::Range<usize>, super::SyncError> {
    let value = raw::first(bytes, true)?;
    let at = raw::space(bytes, 0);
    Ok(at..at + value.0.len())
}
