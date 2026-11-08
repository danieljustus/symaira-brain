//! Layered context assembly (ported for #760 from
//! `internal/memory/contextassembler` at v0.12.0).
//!
//! [`Assembler::assemble`] builds a prompt context under a token budget from
//! layers in priority order: working context, working memory, session
//! summary, profile, expandable session sources and retrieval. A hard budget
//! pass ([`budget`]) drops the lowest-priority pieces and reports what it
//! dropped; an optional snapshot chain yields delta packs ([`delta`]).
//!
//! Reads go through [`source::ContextSource`] (the `Store` implements it as
//! [`source::StoreSource`]); the clock, the embedder and the token estimator
//! are injected, so behavior is deterministic in tests.

// Prompt text is assembled with push_str(format!) for readability.
#![allow(clippy::format_push_string)]

pub mod assembler;
pub mod budget;
pub mod delta;
mod format;
mod layers;
mod profile;
mod receipt;
pub mod source;

#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

pub use assembler::Assembler;
pub use budget::{BudgetReport, default_token_estimate};
pub use delta::{ContentHashSnapshot, DeltaPack};

/// Prompt layer a piece belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLayer {
    /// Recent turns of the live session.
    WorkingContext,
    /// Active working-tier memories.
    WorkingMemory,
    /// Stored session summary.
    Summary,
    /// Semantic retrieval.
    Retrieval,
    /// Synthesized static/dynamic profile.
    Profile,
    /// Expandable session sources.
    SessionContext,
}

impl ContextLayer {
    /// The wire word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkingContext => "working_context",
            Self::WorkingMemory => "working_memory",
            Self::Summary => "summary",
            Self::Retrieval => "retrieval",
            Self::Profile => "profile",
            Self::SessionContext => "session_context",
        }
    }
}

/// One assembled piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssembledPiece {
    /// Layer.
    pub layer: ContextLayer,
    /// Text.
    pub content: String,
    /// Estimated tokens (`-1` in delta packs, which carry no content).
    pub tokens: i64,
    /// Engine-minted recall receipt (#487), when enabled.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub receipt: String,
}

/// An expandable source of the session-context layer (#400).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpandableSource {
    /// Source id.
    pub source_id: String,
    /// `memory` or `session`.
    pub source_type: String,
    /// Short label.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// SHA-256 hex of the source content.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    /// Reference count.
    #[serde(skip_serializing_if = "is_zero")]
    pub ref_count: i64,
    /// Expansion steps available.
    pub expand_steps: i64,
    /// Extra context.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// The assembled context.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssembledContext {
    /// Query.
    pub query: String,
    /// Token ceiling.
    pub budget: i64,
    /// Tokens used by the pieces.
    pub used_tokens: i64,
    /// Pieces in priority order.
    pub pieces: Vec<AssembledPiece>,
    /// Hard-budget report (#492), when a budget was enforced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_report: Option<BudgetReport>,
    /// Session-context sources (#400).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expandable_sources: Vec<ExpandableSource>,
    /// Delta against the previous snapshot (#412).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<DeltaPack>,
}

/// `[context]` settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextConfig {
    /// Max tokens for the assembled context.
    pub token_budget: i64,
    /// Max recent turns of working context.
    pub max_working_turns: i64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            token_budget: 2000,
            max_working_turns: 5,
        }
    }
}

/// `[working_memory]` settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkingMemoryConfig {
    /// Max working memories in the context (<= 0: 50).
    pub max_items: i64,
    /// Whether the layer is included.
    pub include_in_context: bool,
}

/// Degradation ladder settings (#414).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DegradationConfig {
    /// Enable the ladder.
    pub enabled: bool,
    /// Token cap of a full item.
    pub full_tokens: i64,
    /// Token cap of a summary item.
    pub summary_tokens: i64,
    /// Token cap of a reference item.
    pub ref_tokens: i64,
}

impl Default for DegradationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            full_tokens: 200,
            summary_tokens: 80,
            ref_tokens: 30,
        }
    }
}

/// Delta pack settings (#412).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaPackConfig {
    /// Enable snapshots.
    pub enabled: bool,
    /// Snapshots kept per session.
    pub max_snapshots: usize,
    /// Minimum token growth in percent for a new snapshot.
    pub snapshot_on_token_pct: i64,
    /// Minimum seconds between snapshots.
    pub min_snapshot_interval_secs: i64,
}

impl Default for DeltaPackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_snapshots: 10,
            snapshot_on_token_pct: 20,
            min_snapshot_interval_secs: 60,
        }
    }
}

/// Profile layer settings (#403).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileLayerConfig {
    /// Enable the layer.
    pub enabled: bool,
    /// Max chars of static profile fields.
    pub max_static_len: usize,
    /// Token cap of the layer.
    pub token_budget: i64,
}

impl Default for ProfileLayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_static_len: 200,
            token_budget: 150,
        }
    }
}

/// Session-context layer settings (#400).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionContextConfig {
    /// Enable the layer.
    pub enabled: bool,
    /// Max sources.
    pub max_sources: usize,
    /// Token cap of the layer.
    pub token_budget: i64,
    /// Expansion steps per source.
    pub max_expand_per_src: i64,
}

impl Default for SessionContextConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_sources: 5,
            token_budget: 300,
            max_expand_per_src: 3,
        }
    }
}

/// A recalled memory as the assembler needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Recalled {
    /// Memory id.
    pub id: String,
    /// Content.
    pub content: String,
    /// Scope.
    pub scope: String,
    /// Semantic kind.
    pub kind: String,
    /// Tier.
    pub tier: String,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Retrieval score (aging already applied).
    pub score: f32,
}
