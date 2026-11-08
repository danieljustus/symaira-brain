//! Engine-minted recall receipts (`memory/receipt.go`, #487).
//!
//! The engine, not the agent, mints the one-line citation, so an agent cannot
//! fabricate a receipt for a memory that was never returned.

use super::Recalled;
use super::format::prefix;
use super::layers::age;
use chrono::{DateTime, Utc};
use std::fmt::Write as _;

/// How much content a receipt quotes.
const CONTENT_PREFIX: usize = 60;

/// Go's `%q` for the characters a whitespace-normalized prefix can contain.
fn go_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 || u32::from(c) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `◉ memory: "daemon runs on port 8787" (project, 3d)`.
#[must_use]
pub(crate) fn mint(item: &Recalled, now: DateTime<Utc>) -> String {
    let normalized = item
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    format!(
        "◉ memory: {} ({}, {})",
        go_quote(prefix(&normalized, CONTENT_PREFIX)),
        item.scope,
        age(item.created_at, now)
    )
}
