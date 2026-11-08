//! Content-hash snapshots and delta packs (#412).

use super::{AssembledContext, AssembledPiece, ContextLayer, DeltaPackConfig};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

/// A content-hash snapshot of an assembled context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentHashSnapshot {
    /// Session.
    pub session_id: String,
    /// When it was taken.
    pub taken_at: DateTime<Utc>,
    /// Tokens used.
    pub used_tokens: i64,
    /// SHA-256 hex of every piece's content.
    pub piece_hashes: Vec<String>,
    /// Layer of every piece.
    pub layer_order: Vec<ContextLayer>,
    /// `source_id -> content_hash` of the expandable sources.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub source_hashes: BTreeMap<String, String>,
}

/// What changed between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaPack {
    /// Session.
    pub session_id: String,
    /// Key of the prior snapshot.
    pub from_snapshot: String,
    /// Key of this snapshot.
    pub to_snapshot: String,
    /// Layers whose content changed (tokens are `-1`: no content is carried).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added_pieces: Vec<AssembledPiece>,
    /// Layers that disappeared.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed_layers: Vec<ContextLayer>,
    /// Token change.
    pub token_delta: i64,
}

fn sha_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

impl ContentHashSnapshot {
    /// Identity digest: `sha256("<session>|<tokens>|[<hash> <hash>]")`.
    #[must_use]
    pub fn key(&self) -> String {
        sha_hex(&format!(
            "{}|{}|[{}]",
            self.session_id,
            self.used_tokens,
            self.piece_hashes.join(" ")
        ))
    }

    pub(crate) fn of(session_id: &str, ctx: &AssembledContext, taken_at: DateTime<Utc>) -> Self {
        Self {
            session_id: session_id.to_owned(),
            taken_at,
            used_tokens: ctx.used_tokens,
            piece_hashes: ctx.pieces.iter().map(|p| sha_hex(&p.content)).collect(),
            layer_order: ctx.pieces.iter().map(|p| p.layer).collect(),
            source_hashes: ctx
                .expandable_sources
                .iter()
                .filter(|s| !s.content_hash.is_empty())
                .map(|s| (s.source_id.clone(), s.content_hash.clone()))
                .collect(),
        }
    }
}

/// `None` when the snapshots are identical.
pub(crate) fn compute_delta(
    session_id: &str,
    prev: &ContentHashSnapshot,
    curr: &ContentHashSnapshot,
) -> Option<DeltaPack> {
    if prev.key() == curr.key() {
        return None;
    }
    let mut pack = DeltaPack {
        session_id: session_id.to_owned(),
        from_snapshot: prev.key(),
        to_snapshot: curr.key(),
        added_pieces: Vec::new(),
        removed_layers: Vec::new(),
        token_delta: curr.used_tokens - prev.used_tokens,
    };
    let prev_layers: HashSet<_> = prev.layer_order.iter().copied().collect();
    let curr_layers: HashSet<_> = curr.layer_order.iter().copied().collect();
    let mut removed = HashSet::new();
    for layer in &prev.layer_order {
        if !curr_layers.contains(layer) && removed.insert(*layer) {
            pack.removed_layers.push(*layer);
        }
    }
    let marker = |layer| AssembledPiece {
        layer,
        content: String::new(),
        tokens: -1,
        receipt: String::new(),
    };
    for (index, layer) in curr.layer_order.iter().enumerate() {
        if index < prev.layer_order.len() && index < prev.piece_hashes.len() {
            if curr
                .piece_hashes
                .get(index)
                .is_some_and(|h| *h != prev.piece_hashes[index])
            {
                pack.added_pieces.push(marker(*layer));
            }
        } else if !prev_layers.contains(layer) {
            pack.added_pieces.push(marker(*layer));
        }
    }
    Some(pack)
}

/// Appends a snapshot to `chain` when the change is large and recent-enough
/// apart, and returns the delta against the previous one.
pub(crate) fn maybe_snapshot(
    chain: &mut Vec<ContentHashSnapshot>,
    config: &DeltaPackConfig,
    session_id: &str,
    ctx: &AssembledContext,
    now: DateTime<Utc>,
) -> Option<DeltaPack> {
    let curr = ContentHashSnapshot::of(session_id, ctx, now);
    if let Some(last) = chain.last() {
        if curr.used_tokens < last.used_tokens {
            return None;
        }
        let pct = if last.used_tokens > 0 {
            (curr.used_tokens - last.used_tokens) * 100 / last.used_tokens
        } else {
            0
        };
        if pct < config.snapshot_on_token_pct {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let seconds = (curr.taken_at - last.taken_at).num_milliseconds() as f64 / 1000.0;
        #[allow(clippy::cast_precision_loss)]
        if seconds < config.min_snapshot_interval_secs as f64 {
            return None;
        }
    }
    chain.push(curr);
    if chain.len() > config.max_snapshots {
        let excess = chain.len() - config.max_snapshots;
        chain.drain(..excess);
    }
    if chain.len() < 2 {
        return None;
    }
    compute_delta(session_id, &chain[chain.len() - 2], &chain[chain.len() - 1])
}
