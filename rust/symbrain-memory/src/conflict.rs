//! Write-path contradiction detection (`internal/memory/conflict/conflict.go`).
//!
//! Tiers: a byte-identical repeat is deduplicated, near-duplicates are
//! replaced by the newer row, and the contradiction band gets a verdict from
//! the optional LLM tier (ambiguous when absent or failing).

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use crate::conflict_llm::{self, LlmSettings};
use crate::search_rows::{SEARCH_COLUMNS, SearchRow};

/// Relationship between the new memory and one candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Repeat,
    Contradiction,
    Ambiguous,
}

/// One contradiction-band pair handed to the verdict tier.
pub(crate) struct Pair {
    pub cand: SearchRow,
    pub new_content: Vec<u8>,
    pub old_content: Vec<u8>,
    pub new_actor: Vec<u8>,
    pub old_actor: Vec<u8>,
    pub similarity: f32,
}

/// The decided outcome for one candidate.
pub(crate) struct Resolution {
    pub candidate: SearchRow,
    pub similarity: f32,
    pub verdict: Verdict,
    pub winner_id: String,
    pub loser_id: String,
    pub winner_actor: Vec<u8>,
    pub loser_actor: Vec<u8>,
    pub rule: &'static str,
}

/// `config.ConflictConfig` after `normalize`.
#[derive(Clone, Debug)]
pub struct ConflictConfig {
    pub enabled: bool,
    pub contradiction_threshold: f64,
    pub near_dup_threshold: f64,
    pub max_candidates: i64,
    pub llm: LlmSettings,
}

impl Default for ConflictConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            contradiction_threshold: 0.80,
            near_dup_threshold: 0.95,
            max_candidates: 10,
            llm: LlmSettings::default(),
        }
    }
}

impl ConflictConfig {
    fn normalized(&self) -> Self {
        let mut config = self.clone();
        if config.contradiction_threshold <= 0.0 || config.contradiction_threshold >= 1.0 {
            config.contradiction_threshold = 0.80;
        }
        if config.near_dup_threshold <= 0.0 || config.near_dup_threshold >= 1.0 {
            config.near_dup_threshold = 0.95;
        }
        if config.max_candidates <= 0 {
            config.max_candidates = 10;
        }
        config
    }
}

/// The memory being written, as the checker sees it.
pub(crate) struct NewMemory<'a> {
    pub id: &'a str,
    pub content: &'a [u8],
    pub content_hash: &'a str,
    pub scope: &'a str,
    pub embedding: &'a [f32],
    pub embedding_source: &'a str,
    pub created_by: &'a [u8],
    pub created_at: DateTime<Utc>,
    pub importance: f64,
}

fn load(conn: &Connection, id: &str) -> rusqlite::Result<Option<SearchRow>> {
    conn.query_row(
        &format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE id = ?"),
        [id],
        SearchRow::from_row,
    )
    .optional()
}

/// `FindActiveDuplicate`.
fn find_active_duplicate(
    conn: &Connection,
    hash: &str,
    scope: &str,
) -> rusqlite::Result<Option<SearchRow>> {
    if hash.is_empty() {
        return Ok(None);
    }
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM memories
             WHERE content_hash = ? AND scope = ?
               AND consolidation_status != 'archived'
               AND review_status = 'approved'
               AND retired_at IS NULL
               AND (superseded_by IS NULL OR superseded_by = '')
             ORDER BY created_at DESC LIMIT 1",
            [hash, scope],
            |row| row.get(0),
        )
        .optional()?;
    match id {
        Some(id) => load(conn, &id),
        None => Ok(None),
    }
}

fn to_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// `policyWinner`: importance, then recency, then the lower id.
fn policy_winner(new: &NewMemory<'_>, candidate: &SearchRow) -> (bool, &'static str) {
    #[allow(clippy::float_cmp)]
    if new.importance != candidate.importance {
        return (new.importance > candidate.importance, "importance");
    }
    let candidate_created = candidate.created_at.unwrap_or_default();
    if new.created_at != candidate_created {
        return (new.created_at > candidate_created, "recency");
    }
    (new.id < candidate.id.as_str(), "id")
}

/// `Checker.Check`. The error text is what the shipped checker wraps.
pub(crate) fn check(
    conn: &Connection,
    config: &ConflictConfig,
    new: &NewMemory<'_>,
    prefilter: bool,
    observe: crate::retrieval::Observe<'_>,
    log: &mut crate::slog::LogSink<'_>,
) -> Result<Vec<Resolution>, String> {
    let config = config.normalized();
    if !config.enabled || new.embedding.is_empty() || new.embedding_source.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(duplicate) = find_active_duplicate(conn, new.content_hash, new.scope)
        .map_err(|error| format!("conflict: duplicate lookup: {error}"))?
    {
        return Ok(vec![Resolution {
            candidate: duplicate,
            similarity: 1.0,
            verdict: Verdict::Repeat,
            winner_id: String::new(),
            loser_id: String::new(),
            winner_actor: Vec::new(),
            loser_actor: Vec::new(),
            rule: "",
        }]);
    }
    let limit = usize::try_from(config.max_candidates).unwrap_or(10);
    let results = crate::retrieval::search_with(
        conn,
        &crate::retrieval::Query {
            vector: new.embedding,
            source: new.embedding_source,
            scope: new.scope,
            limit,
            prefilter,
            filters: &crate::SearchFilters::default(),
            observe,
        },
    )
    .map_err(|error| format!("conflict: candidate recall: {error}"))?;

    let near_dup = narrow(config.near_dup_threshold);
    let contradiction = narrow(config.contradiction_threshold);
    let mut best_near_dup: Option<Pair> = None;
    let mut band: Vec<Pair> = Vec::new();
    for result in results {
        let candidate = result.memory;
        if candidate.id == new.id
            || !candidate.superseded_by.is_empty()
            || candidate.embedding.is_empty()
        {
            continue;
        }
        let similarity = crate::retrieval::cosine_similarity(new.embedding, &candidate.embedding);
        let pair = |candidate: SearchRow| Pair {
            new_content: new.content.to_vec(),
            old_content: to_bytes(&candidate.content),
            new_actor: new.created_by.to_vec(),
            old_actor: to_bytes(&candidate.created_by),
            similarity,
            cand: candidate,
        };
        if similarity >= near_dup {
            if best_near_dup
                .as_ref()
                .is_none_or(|best| similarity > best.similarity)
            {
                best_near_dup = Some(pair(candidate));
            }
        } else if similarity >= contradiction {
            band.push(pair(candidate));
        }
    }
    if let Some(pair) = best_near_dup {
        return Ok(vec![Resolution {
            winner_actor: new.created_by.to_vec(),
            loser_actor: pair.old_actor.clone(),
            winner_id: new.id.to_owned(),
            loser_id: pair.cand.id.clone(),
            similarity: pair.similarity,
            verdict: Verdict::Contradiction,
            rule: "near_dup",
            candidate: pair.cand,
        }]);
    }
    if band.is_empty() {
        return Ok(Vec::new());
    }

    let verdicts = if config.llm.provider.is_empty() {
        None
    } else {
        match conflict_llm::verdicts(&config.llm, &band) {
            Ok(verdicts) if verdicts.len() == band.len() => Some(verdicts),
            Ok(_) => {
                warn_degraded(log, band.len(), "<nil>");
                None
            }
            Err(error) => {
                warn_degraded(log, band.len(), &error);
                None
            }
        }
    };
    let verdicts = verdicts.unwrap_or_else(|| vec![Verdict::Ambiguous; band.len()]);

    let mut resolutions = Vec::with_capacity(band.len());
    for (pair, verdict) in band.into_iter().zip(verdicts) {
        let old_actor = pair.old_actor.clone();
        let similarity = pair.similarity;
        let mut resolution = Resolution {
            candidate: pair.cand,
            similarity,
            verdict: Verdict::Ambiguous,
            winner_id: String::new(),
            loser_id: String::new(),
            winner_actor: Vec::new(),
            loser_actor: Vec::new(),
            rule: "",
        };
        match verdict {
            Verdict::Contradiction => {
                let (new_wins, rule) = policy_winner(new, &resolution.candidate);
                resolution.verdict = Verdict::Contradiction;
                resolution.rule = rule;
                let candidate_id = resolution.candidate.id.clone();
                if new_wins {
                    resolution.winner_id = new.id.to_owned();
                    resolution.loser_id = candidate_id;
                    resolution.winner_actor = new.created_by.to_vec();
                    resolution.loser_actor = old_actor;
                } else {
                    resolution.winner_id = candidate_id;
                    resolution.loser_id = new.id.to_owned();
                    resolution.winner_actor = old_actor;
                    resolution.loser_actor = new.created_by.to_vec();
                }
            }
            Verdict::Repeat => {
                resolution.verdict = Verdict::Contradiction;
                resolution.rule = "repeat";
                resolution.winner_id = new.id.to_owned();
                resolution.loser_id = resolution.candidate.id.clone();
                resolution.winner_actor = new.created_by.to_vec();
                resolution.loser_actor = old_actor;
            }
            Verdict::Ambiguous => {}
        }
        resolutions.push(resolution);
    }
    Ok(resolutions)
}

/// `float32(threshold)`.
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: f64) -> f32 {
    value as f32
}

fn warn_degraded(log: &mut crate::slog::LogSink<'_>, pairs: usize, error: &str) {
    log.warn(
        "conflict: verdict tier failed, storing both",
        &[("pairs", &pairs.to_string()), ("error", error)],
    );
}
