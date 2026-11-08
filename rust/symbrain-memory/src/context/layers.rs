//! Layer builders of the assembler: profile, expandable session sources and
//! retrieval with the degradation ladder.

use super::assembler::{Assembler, kind_rank, score_band};
use super::format::{Level, degraded_block, retrieval_block, single_result, truncate};
use super::profile::{format_profile, synthesize};
use super::{
    AssembledPiece, ContextLayer, ExpandableSource, ProfileLayerConfig, Recalled,
    SessionContextConfig,
};
use crate::pii::redact;
use crate::write;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;

impl Assembler<'_> {
    pub(super) fn profile_text(&self, config: &ProfileLayerConfig, session_id: &str) -> String {
        let Ok(mut cache) = self.cached_profile.lock() else {
            return String::new();
        };
        let profile = cache.get_or_insert_with(|| synthesize(self.source, session_id));
        format_profile(profile, config)
    }

    pub(super) fn session_sources(
        &self,
        config: &SessionContextConfig,
        session_id: &str,
    ) -> Vec<ExpandableSource> {
        let (Some(source), false) = (self.source, session_id.is_empty()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Ok(summary) = source.session_summary(session_id)
            && !summary.is_empty()
        {
            out.push(ExpandableSource {
                source_id: session_id.to_owned(),
                source_type: "session".into(),
                label: "Session summary".into(),
                content_hash: write::content_hash(&summary),
                ref_count: 1,
                expand_steps: config.max_expand_per_src,
                metadata: BTreeMap::from([("type".to_owned(), "summary".to_owned())]),
            });
        }
        if let Ok(memories) = source.keyword_search("", 5) {
            for memory in memories {
                if out.len() >= config.max_sources {
                    break;
                }
                out.push(ExpandableSource {
                    source_id: memory.id,
                    source_type: "memory".into(),
                    label: truncate(&memory.content, 60),
                    content_hash: write::content_hash(&memory.content),
                    ref_count: 1,
                    expand_steps: config.max_expand_per_src,
                    metadata: BTreeMap::from([
                        ("scope".to_owned(), memory.scope),
                        ("tier".to_owned(), memory.tier),
                    ]),
                });
            }
        }
        out
    }

    pub(super) fn retrieve(&self, query: &str, token_budget: i64) -> Option<Vec<Recalled>> {
        let (embed, source) = (self.embedder.as_ref()?, self.source?);
        let (vector, space) = embed(query);
        let limit = usize::try_from((token_budget / 50).clamp(1, 20)).unwrap_or(1);
        let mut results = source.vector_search(&vector, &space, limit).ok()?;
        // Defense in depth: prompt context never carries a raw credential (#515).
        for item in &mut results {
            item.content = redact(&item.content);
        }
        Some(results)
    }

    pub(super) fn fill_retrieval(&self, results: &[Recalled], budget: i64) -> Vec<AssembledPiece> {
        if results.is_empty() || budget <= 0 {
            return Vec::new();
        }
        let Some(ladder) = self.degradation.filter(|d| d.enabled) else {
            let content = retrieval_block(results);
            let tokens = self.estimate(&content);
            if tokens <= budget {
                return vec![AssembledPiece {
                    layer: ContextLayer::Retrieval,
                    content,
                    tokens,
                    receipt: String::new(),
                }];
            }
            let (content, tokens) = degraded_block(results, budget);
            return vec![AssembledPiece {
                layer: ContextLayer::Retrieval,
                content,
                tokens,
                receipt: String::new(),
            }];
        };
        let mut sorted = results.to_vec();
        sorted.sort_by(|a, b| {
            score_band(b.score)
                .cmp(&score_band(a.score))
                .then_with(|| kind_rank(&a.kind).cmp(&kind_rank(&b.kind)))
                .then_with(|| b.score.total_cmp(&a.score))
        });
        let mut used = 0;
        let mut pieces = Vec::new();
        for item in &sorted {
            if used >= budget {
                break;
            }
            let remaining = budget - used;
            for (level, cap) in [
                (Level::Full, ladder.full_tokens),
                (Level::Summary, ladder.summary_tokens),
                (Level::Reference, ladder.ref_tokens),
            ] {
                let content = single_result(item, level);
                let tokens = self.estimate(&content);
                if tokens <= remaining && tokens <= cap {
                    pieces.push(AssembledPiece {
                        layer: ContextLayer::Retrieval,
                        content,
                        tokens,
                        receipt: self.receipt(item),
                    });
                    used += tokens;
                    break;
                }
            }
        }
        pieces
    }
}

pub(super) fn format_sources(sources: &[ExpandableSource]) -> String {
    let mut out = String::from("Session context sources:\n");
    for source in sources {
        let hash = if source.content_hash.is_empty() {
            String::new()
        } else {
            format!(" [{}]", super::format::prefix(&source.content_hash, 8))
        };
        out.push_str(&format!(
            "- [{}] {} (type: {}, expands: {}){hash}\n",
            source.source_id, source.label, source.source_type, source.expand_steps
        ));
    }
    out
}

/// Age of `created` relative to `now` for receipts, kept here so the
/// duration math sits next to its only caller.
pub(crate) fn age(created: DateTime<Utc>, now: DateTime<Utc>) -> String {
    use chrono::Datelike;
    if created.year() <= 1 {
        return "unknown".into();
    }
    let d: Duration = now - created;
    if d < Duration::minutes(1) {
        "just now".into()
    } else if d < Duration::hours(1) {
        format!("{}m", d.num_minutes())
    } else if d < Duration::hours(24) {
        format!("{}h", d.num_hours())
    } else {
        format!("{}d", d.num_hours() / 24)
    }
}
