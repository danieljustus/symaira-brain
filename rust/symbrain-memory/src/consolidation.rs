//! Memory consolidation engine (ported for #760 from
//! `internal/memory/consolidation/engine.go` at v0.12.0).
//!
//! Raw memories are grouped by scope; a scope with one memory is just marked
//! consolidated, otherwise an LLM merges/discards entries and the result is
//! applied in one transaction (new rows, evidence reparenting, archiving of
//! the originals). LLM, embedder, clock and id source are injected so runs are
//! deterministic in tests. Scopes whose LLM call or parse fails are skipped;
//! only a run in which every scope failed is an error.
//!
//! Deviations from Go: scopes are processed in sorted order (Go iterated a
//! map), and a one-memory scope updates `consolidation_status`/`updated_at`
//! in place instead of re-saving the whole row.

pub use crate::consolidation_apply::hash_embed;
use crate::consolidation_apply::{NewRow, report_failures, save_new, update_status};
use crate::consolidation_parse::{ConsolidationResult, parse_response};
use crate::consolidation_prompts::{
    PromptEntry, build_memory_prompt, build_system_prompt, build_user_prompt, resolve_family,
};
use crate::gotime;
use crate::model::{Store, StoreError};
use crate::pii;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Provenance flag set on every synthesized memory (`security.UntrustedContentKey`).
pub const UNTRUSTED_CONTENT_KEY: &str = "source_untrusted";

/// LLM call: `(system, user) -> raw reply`.
pub type LlmFn<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;
/// Embedding call: `text -> (vector, source, model)`.
pub type EmbedFn<'a> = &'a dyn Fn(&str) -> (Vec<f32>, String, String);

/// Injected collaborators and configuration of a run.
pub struct Engine<'a> {
    /// Chat completion with the consolidation schema.
    pub llm: LlmFn<'a>,
    /// Embedding of new content.
    pub embed: EmbedFn<'a>,
    /// Redact PII from synthesized content.
    pub pii_enabled: bool,
    /// Configured default prompt family (`chat`/`code`).
    pub prompt_mode: String,
    /// Clock.
    pub now: &'a dyn Fn() -> DateTime<Utc>,
    /// Id source for new memories (UUID v4 in production).
    pub new_id: &'a dyn Fn() -> String,
}

/// A memory the engine reads.
#[derive(Debug, Clone)]
pub(crate) struct RawMemory {
    pub id: String,
    pub content: String,
    pub scope: String,
    pub metadata: Map<String, Value>,
    pub created_at: DateTime<Utc>,
}

/// A memory the run created (or, for a lone raw memory, re-flagged).
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedMemory {
    /// Row id.
    pub id: String,
    /// Content (redacted when PII redaction is on).
    pub content: String,
    /// Scope.
    pub scope: String,
    /// String metadata incl. the untrusted-provenance flag.
    pub metadata: BTreeMap<String, String>,
    /// Always `consolidated`.
    pub consolidation_status: String,
}

/// What happened (or would happen on a dry run) in one scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScopeChangeSummary {
    /// The scope.
    pub scope: String,
    /// Created memories.
    pub new_memories: Vec<ConsolidatedMemory>,
    /// Ids archived (replaced or discarded), in encounter order.
    pub archived_memory_ids: Vec<String>,
    /// Replaced id -> id of the memory that replaced it.
    pub replaced_id_to_new_id: BTreeMap<String, String>,
}

const MEMORY_COLUMNS: &str = "id, content, scope, metadata, created_at";

fn read_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMemory> {
    let created: String = row.get(4)?;
    let metadata: String = row.get(3)?;
    Ok(RawMemory {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        metadata: serde_json::from_str(&metadata).unwrap_or_default(),
        created_at: gotime::parse(&created).unwrap_or_default(),
    })
}

fn wrap(context: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(format!("{context}: {error}"))
}

impl Store {
    fn raw_memories(&self) -> Result<Vec<RawMemory>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(&format!(
            "SELECT {MEMORY_COLUMNS} FROM memories WHERE consolidation_status = 'raw' AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = statement.query_map([], read_raw)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn memories_by_ids(&self, ids: &[String]) -> Result<Vec<RawMemory>, StoreError> {
        let conn = self.lock()?;
        let mut out = Vec::new();
        for id in ids {
            let found = conn
                .query_row(
                    &format!("SELECT {MEMORY_COLUMNS} FROM memories WHERE id = ?"),
                    [id],
                    read_raw,
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            out.extend(found);
        }
        Ok(out)
    }

    /// `Engine.RunConsolidation`: consolidates every raw memory (optionally one
    /// scope). With `dry_run` nothing is written or journaled.
    ///
    /// # Errors
    /// A SQLite error, or a Go-worded error when every scope failed.
    pub fn run_consolidation(
        &self,
        engine: &Engine<'_>,
        scope_filter: &str,
        dry_run: bool,
    ) -> Result<Vec<ScopeChangeSummary>, StoreError> {
        let raw = self.raw_memories()?;
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let raw = raw
            .into_iter()
            .filter(|m| scope_filter.is_empty() || m.scope == scope_filter)
            .collect();
        self.consolidate_groups(engine, raw, dry_run)
    }

    /// `Engine.RunConsolidationForMemories` for working-memory compaction:
    /// consolidates the given memory ids regardless of their status.
    ///
    /// # Errors
    /// As [`Store::run_consolidation`].
    pub fn run_consolidation_for_memories(
        &self,
        engine: &Engine<'_>,
        ids: &[String],
        dry_run: bool,
    ) -> Result<Vec<ScopeChangeSummary>, StoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let memories = self.memories_by_ids(ids)?;
        self.consolidate_groups(engine, memories, dry_run)
    }

    fn consolidate_groups(
        &self,
        engine: &Engine<'_>,
        memories: Vec<RawMemory>,
        dry_run: bool,
    ) -> Result<Vec<ScopeChangeSummary>, StoreError> {
        let mut grouped: BTreeMap<String, Vec<RawMemory>> = BTreeMap::new();
        for memory in memories {
            grouped
                .entry(memory.scope.clone())
                .or_default()
                .push(memory);
        }
        let mut summaries = Vec::new();
        let mut failures: Vec<(String, String)> = Vec::new();
        for (scope, group) in grouped {
            if group.len() <= 1 {
                summaries.push(self.mark_single(engine, &scope, &group[0], dry_run)?);
                continue;
            }
            match self.consolidate_scope(engine, &scope, &group, dry_run) {
                Ok(summary) => summaries.push(summary),
                Err(ScopeError::Skip(reason)) => failures.push((scope, reason)),
                Err(ScopeError::Fatal(error)) => return Err(error),
            }
        }
        report_failures(&failures, summaries.len())?;
        self.journal_run(engine, dry_run, &summaries);
        Ok(summaries)
    }

    fn mark_single(
        &self,
        engine: &Engine<'_>,
        scope: &str,
        memory: &RawMemory,
        dry_run: bool,
    ) -> Result<ScopeChangeSummary, StoreError> {
        let mut summary = ScopeChangeSummary {
            scope: scope.to_owned(),
            ..ScopeChangeSummary::default()
        };
        summary.new_memories.push(ConsolidatedMemory {
            id: memory.id.clone(),
            content: memory.content.clone(),
            scope: memory.scope.clone(),
            metadata: memory
                .metadata
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect(),
            consolidation_status: "consolidated".into(),
        });
        summary.archived_memory_ids.push(memory.id.clone());
        summary
            .replaced_id_to_new_id
            .insert(memory.id.clone(), memory.id.clone());
        if !dry_run {
            let conn = self.lock()?;
            conn.execute(
                "UPDATE memories SET consolidation_status = 'consolidated', updated_at = ? WHERE id = ?",
                params![gotime::format((engine.now)()), memory.id],
            )
            .map_err(|e| wrap("failed to update raw memory to consolidated", e))?;
        }
        Ok(summary)
    }

    fn consolidate_scope(
        &self,
        engine: &Engine<'_>,
        scope: &str,
        group: &[RawMemory],
        dry_run: bool,
    ) -> Result<ScopeChangeSummary, ScopeError> {
        let result = ask_llm(engine, scope, group).map_err(ScopeError::Skip)?;
        let mut summary = ScopeChangeSummary {
            scope: scope.to_owned(),
            ..ScopeChangeSummary::default()
        };
        let mut rows: Vec<NewRow> = Vec::new();
        for item in &result.consolidated {
            let content = if engine.pii_enabled {
                pii::redact(&item.content)
            } else {
                item.content.clone()
            };
            let (vector, source, model) = (engine.embed)(&content);
            let id = (engine.new_id)();
            let mut metadata = item.metadata.clone();
            metadata.insert(UNTRUSTED_CONTENT_KEY.into(), "true".into());
            let memory = ConsolidatedMemory {
                id: id.clone(),
                content,
                scope: scope.to_owned(),
                metadata,
                consolidation_status: "consolidated".into(),
            };
            rows.push(NewRow {
                memory: memory.clone(),
                vector,
                source,
                model,
                now: (engine.now)(),
            });
            summary.new_memories.push(memory);
            for replaced in &item.replaces_ids {
                summary.archived_memory_ids.push(replaced.clone());
                summary
                    .replaced_id_to_new_id
                    .insert(replaced.clone(), id.clone());
            }
        }
        summary
            .archived_memory_ids
            .extend(result.discarded_ids.iter().cloned());
        if !dry_run {
            self.apply(engine, group, &result, &summary, &rows)
                .map_err(ScopeError::Fatal)?;
        }
        Ok(summary)
    }

    fn apply(
        &self,
        engine: &Engine<'_>,
        group: &[RawMemory],
        result: &ConsolidationResult,
        summary: &ScopeChangeSummary,
        rows: &[NewRow],
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| wrap("failed to begin transaction", e))?;
        for row in rows {
            save_new(&tx, row).map_err(|e| wrap("failed to save consolidated memory", e))?;
        }
        for (old, new) in &summary.replaced_id_to_new_id {
            crate::evidence_store::reparent_memory_evidence_tx(&tx, old, new).map_err(|e| {
                StoreError::Invalid(format!(
                    "failed to propagate evidence from {old} to {new}: {e}"
                ))
            })?;
        }
        for memory in group {
            let parent = summary.replaced_id_to_new_id.get(&memory.id);
            let discarded = result.discarded_ids.contains(&memory.id);
            // Memories the model neither replaced nor discarded stay raw.
            if parent.is_none() && !discarded {
                continue;
            }
            let parent = if discarded { None } else { parent };
            update_status(
                &tx,
                &memory.id,
                "archived",
                parent.map(String::as_str),
                (engine.now)(),
            )
            .map_err(|e| {
                StoreError::Invalid(format!(
                    "failed to archive original memory {}: {e}",
                    memory.id
                ))
            })?;
        }
        tx.commit()
            .map_err(|e| wrap("failed to commit transaction", e))
    }
}

enum ScopeError {
    /// LLM or parse failure: skip the scope.
    Skip(String),
    /// Storage failure: abort the run.
    Fatal(StoreError),
}

fn ask_llm(
    engine: &Engine<'_>,
    scope: &str,
    group: &[RawMemory],
) -> Result<ConsolidationResult, String> {
    let entries: Vec<PromptEntry<'_>> = group
        .iter()
        .map(|m| PromptEntry {
            content: &m.content,
            created_at: m.created_at,
        })
        .collect();
    let content = build_memory_prompt(&entries);
    let metadata: Vec<&Map<String, Value>> = group.iter().map(|m| &m.metadata).collect();
    let family = resolve_family(&metadata, &engine.prompt_mode);
    let raw = (engine.llm)(
        &build_system_prompt(family),
        &build_user_prompt(family, scope, &content),
    )?;
    let mut result = parse_response(&raw)?;
    let lookup = |refs: &[String]| -> Vec<String> {
        refs.iter()
            .filter_map(|r| r.parse::<usize>().ok().filter(|n| r == &n.to_string()))
            .filter_map(|n| n.checked_sub(1).and_then(|i| group.get(i)))
            .map(|m| m.id.clone())
            .collect()
    };
    for item in &mut result.consolidated {
        item.replaces_ids = lookup(&item.replaces_ids);
    }
    result.discarded_ids = lookup(&result.discarded_ids);
    Ok(result)
}
