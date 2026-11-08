//! The assembler itself (`assembler.go`).

use super::budget::{default_token_estimate, enforce};
use super::delta::{ContentHashSnapshot, maybe_snapshot};
use super::format::{working_block, working_context};
use super::layers::format_sources;
use super::profile::Profile;
use super::source::ContextSource;
use super::{
    AssembledContext, AssembledPiece, ContextConfig, ContextLayer, DegradationConfig,
    DeltaPackConfig, ProfileLayerConfig, Recalled, SessionContextConfig, WorkingMemoryConfig,
};
use crate::model::StoreError;
use crate::pii::redact;
use crate::summarizer::summarize_session;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Mutex;

type Estimator<'a> = Box<dyn Fn(&str) -> i64 + 'a>;
/// Text to `(vector, embedding source)`.
type Embedder<'a> = Box<dyn Fn(&str) -> (Vec<f32>, String) + 'a>;

/// Builds [`AssembledContext`]s.
pub struct Assembler<'a> {
    pub(super) source: Option<&'a dyn ContextSource>,
    pub(super) embedder: Option<Embedder<'a>>,
    pub(super) estimator: Option<Estimator<'a>>,
    pub(super) now: &'a dyn Fn() -> DateTime<Utc>,
    pub(super) config: ContextConfig,
    pub(super) working: Option<WorkingMemoryConfig>,
    pub(super) degradation: Option<DegradationConfig>,
    pub(super) delta: Option<DeltaPackConfig>,
    pub(super) profile: Option<ProfileLayerConfig>,
    pub(super) session: Option<SessionContextConfig>,
    pub(super) recall_receipts: bool,
    pub(super) snapshots: Mutex<HashMap<String, Vec<ContentHashSnapshot>>>,
    pub(super) cached_profile: Mutex<Option<Profile>>,
}

pub(super) fn kind_rank(kind: &str) -> u8 {
    match kind {
        "user" => 0,
        "feedback" => 1,
        "project" => 2,
        "reference" => 3,
        _ => 4,
    }
}

/// Quantizes a score into 0.05-wide bands (#486).
pub(super) fn score_band(score: f32) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let band = (f64::from(score) * 20.0).round() as i64;
    band
}

impl<'a> Assembler<'a> {
    /// An assembler reading from `source` (none: only the working-context
    /// layer is available) with an injected clock.
    #[must_use]
    pub fn new(
        source: Option<&'a dyn ContextSource>,
        config: ContextConfig,
        now: &'a dyn Fn() -> DateTime<Utc>,
    ) -> Self {
        Self {
            source,
            embedder: None,
            estimator: None,
            now,
            config,
            working: None,
            degradation: None,
            delta: None,
            profile: None,
            session: None,
            recall_receipts: false,
            snapshots: Mutex::new(HashMap::new()),
            cached_profile: Mutex::new(None),
        }
    }

    /// Enables semantic retrieval with this embedder.
    #[must_use]
    pub fn with_embedder(mut self, embed: impl Fn(&str) -> (Vec<f32>, String) + 'a) -> Self {
        self.embedder = Some(Box::new(embed));
        self
    }

    /// Plugs in a real tokenizer for budget enforcement (#492).
    #[must_use]
    pub fn with_token_estimator(mut self, estimate: impl Fn(&str) -> i64 + 'a) -> Self {
        self.estimator = Some(Box::new(estimate));
        self
    }

    /// Sets the working-memory layer (`SetWorkingMemoryConfig`).
    #[must_use]
    pub const fn with_working_memory(mut self, config: WorkingMemoryConfig) -> Self {
        self.working = Some(config);
        self
    }

    /// Sets the degradation ladder (#414).
    #[must_use]
    pub const fn with_degradation(mut self, config: DegradationConfig) -> Self {
        self.degradation = Some(config);
        self
    }

    /// Sets delta packs (#412).
    #[must_use]
    pub const fn with_delta_packs(mut self, config: DeltaPackConfig) -> Self {
        self.delta = Some(config);
        self
    }

    /// Sets the profile layer (#403).
    #[must_use]
    pub const fn with_profile_layer(mut self, config: ProfileLayerConfig) -> Self {
        self.profile = Some(config);
        self
    }

    /// Sets the expandable session context (#400).
    #[must_use]
    pub const fn with_session_context(mut self, config: SessionContextConfig) -> Self {
        self.session = Some(config);
        self
    }

    /// Mints recall receipts on retrieval pieces (#487).
    #[must_use]
    pub const fn with_recall_receipts(mut self, enabled: bool) -> Self {
        self.recall_receipts = enabled;
        self
    }

    /// Snapshot chain of a session (read-only copy).
    #[must_use]
    pub fn snapshots(&self, session_id: &str) -> Vec<ContentHashSnapshot> {
        self.snapshots
            .lock()
            .map(|map| map.get(session_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    pub(super) fn estimate(&self, text: &str) -> i64 {
        self.estimator
            .as_ref()
            .map_or_else(|| default_token_estimate(text), |f| f(text))
    }

    pub(super) fn receipt(&self, item: &Recalled) -> String {
        if self.recall_receipts {
            crate::context::receipt::mint(item, (self.now)())
        } else {
            String::new()
        }
    }

    pub(super) fn piece(&self, layer: ContextLayer, content: String) -> AssembledPiece {
        AssembledPiece {
            layer,
            tokens: self.estimate(&content),
            content,
            receipt: String::new(),
        }
    }

    #[allow(clippy::too_many_lines)] // one linear pass over the seven layers, like Go
    /// Assembles the context for `query` and the live `session_text`.
    ///
    /// # Errors
    /// Never fails today (layer read errors drop the layer, like Go); the
    /// `Result` keeps the Go signature.
    pub fn assemble(
        &self,
        query: &str,
        session_text: &str,
        session_id: &str,
    ) -> Result<AssembledContext, StoreError> {
        let budget = if self.config.token_budget > 0 {
            self.config.token_budget
        } else {
            2000
        };
        let mut ctx = AssembledContext {
            query: query.to_owned(),
            budget,
            used_tokens: 0,
            pieces: Vec::new(),
            budget_report: None,
            expandable_sources: Vec::new(),
            delta: None,
        };
        let mut used = 0;
        let push = |ctx: &mut AssembledContext, piece: AssembledPiece, used: &mut i64| {
            *used += piece.tokens;
            ctx.pieces.push(piece);
        };

        // 1. Working context.
        if !session_text.is_empty() && self.config.max_working_turns > 0 {
            let piece = self.piece(
                ContextLayer::WorkingContext,
                working_context(session_text, self.config.max_working_turns),
            );
            if used + piece.tokens <= budget {
                push(&mut ctx, piece, &mut used);
            }
        }

        // 2. Working memory.
        if let (Some(config), Some(source)) = (self.working, self.source)
            && config.include_in_context
            && used < budget
        {
            let max_items = if config.max_items <= 0 {
                50
            } else {
                config.max_items
            };
            let limit = usize::try_from(max_items).unwrap_or(50);
            if let Ok(mut items) = source.working_memories(limit)
                && !items.is_empty()
            {
                for item in &mut items {
                    item.content = redact(&item.content);
                }
                let piece = self.piece(ContextLayer::WorkingMemory, working_block(&items));
                if used + piece.tokens <= budget {
                    push(&mut ctx, piece, &mut used);
                }
            }
        }

        // 3. Summary.
        if !session_id.is_empty()
            && let Some(source) = self.source
            && let Ok(summary) = source.session_summary(session_id)
            && !summary.is_empty()
        {
            let piece = self.piece(ContextLayer::Summary, summary);
            if used + piece.tokens <= budget {
                push(&mut ctx, piece, &mut used);
            }
        }

        // 4. Profile (needs no query).
        if let Some(config) = self.profile
            && config.enabled
            && used < budget
        {
            let content = self.profile_text(&config, session_id);
            if !content.is_empty() {
                let piece = self.piece(ContextLayer::Profile, content);
                if piece.tokens <= config.token_budget && used + piece.tokens <= budget {
                    push(&mut ctx, piece, &mut used);
                }
            }
        }

        // 5. Expandable session context.
        if let Some(config) = self.session
            && config.enabled
            && used < budget
        {
            let sources = self.session_sources(&config, session_id);
            if !sources.is_empty() {
                let piece = self.piece(ContextLayer::SessionContext, format_sources(&sources));
                if piece.tokens <= config.token_budget && used + piece.tokens <= budget {
                    push(&mut ctx, piece, &mut used);
                    ctx.expandable_sources = sources;
                }
            }
        }

        // 6. Retrieval with the degradation ladder.
        if !query.is_empty() && used < budget {
            let remaining = budget - used;
            if let Some(results) = self.retrieve(query, remaining) {
                for piece in self.fill_retrieval(&results, remaining) {
                    if used + piece.tokens <= budget {
                        push(&mut ctx, piece, &mut used);
                    }
                }
            }
        }

        ctx.used_tokens = used;
        enforce(&mut ctx, budget);

        // 7. Delta pack.
        if !session_id.is_empty()
            && let Some(config) = self.delta
            && config.enabled
            && let Ok(mut map) = self.snapshots.lock()
        {
            let chain = map.entry(session_id.to_owned()).or_default();
            ctx.delta = maybe_snapshot(chain, &config, session_id, &ctx, (self.now)());
        }
        Ok(ctx)
    }

    /// Summarizes `session_text` into at most five bullets and stores it.
    ///
    /// # Errors
    /// The summary could not be saved.
    pub fn produce_session_summary(
        &self,
        session_text: &str,
        session_id: &str,
    ) -> Result<String, StoreError> {
        if session_text.is_empty() {
            return Ok(String::new());
        }
        let summary = summarize_session(session_text, 5);
        let Some(source) = self.source else {
            return Err(StoreError::Invalid(
                "failed to save session summary: no store".into(),
            ));
        };
        source
            .save_session_summary(session_id, &summary)
            .map_err(|e| StoreError::Invalid(format!("failed to save session summary: {e}")))?;
        Ok(summary)
    }
}
