//! Consolidation run journal and undo (ported for #760 from
//! `internal/memory/consolidation/journal.go` and
//! `internal/memory/db/consolidation_runs.go` at v0.12.0).
//!
//! Every non-dry run is recorded in `consolidation_runs` with the ids it
//! created, archived and the replaced->new evidence mapping, so the last
//! completed run can be reversed.

use crate::consolidation::{Engine, ScopeChangeSummary};
use crate::consolidation_apply::update_status;
use crate::gotime;
use crate::model::{Store, StoreError};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The JSON payload stored in `summary_json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSummary {
    /// Ids of memories the run created.
    #[serde(default)]
    pub new_memory_ids: Vec<String>,
    /// Ids the run archived.
    #[serde(default)]
    pub archived_ids: Vec<String>,
    /// Replaced id -> new id, for evidence reparenting.
    #[serde(default)]
    pub replaced_to_new: BTreeMap<String, String>,
}

/// A journaled run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidationRun {
    /// Run id.
    pub id: String,
    /// Run time as stored.
    pub run_at: String,
    /// `completed` or `undone`.
    pub status: String,
    /// Serialized [`RunSummary`].
    pub summary_json: String,
    /// Archived count.
    pub total_archived: i64,
    /// Created count.
    pub total_consolidated: i64,
}

/// `buildRunSummary`: de-duplicated, order-preserving union of the scopes.
#[must_use]
pub fn build_run_summary(summaries: &[ScopeChangeSummary]) -> RunSummary {
    let mut run = RunSummary::default();
    for summary in summaries {
        for memory in &summary.new_memories {
            if !run.new_memory_ids.contains(&memory.id) {
                run.new_memory_ids.push(memory.id.clone());
            }
        }
        for id in &summary.archived_memory_ids {
            if !run.archived_ids.contains(id) {
                run.archived_ids.push(id.clone());
            }
        }
        run.replaced_to_new
            .extend(summary.replaced_id_to_new_id.clone());
    }
    run
}

impl Store {
    /// `journalRun`: best effort, no-op for dry runs and empty results.
    pub(crate) fn journal_run(
        &self,
        engine: &Engine<'_>,
        dry_run: bool,
        summaries: &[ScopeChangeSummary],
    ) {
        if dry_run || summaries.is_empty() {
            return;
        }
        let run = build_run_summary(summaries);
        let summary_json = match serde_json::to_string(&run) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("consolidation: failed to marshal run summary for journal: {error}");
                return;
            }
        };
        let saved = self.lock().and_then(|conn| {
            conn.execute(
                "INSERT INTO consolidation_runs (id, run_at, status, summary_json, total_archived, total_consolidated) VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    (engine.new_id)(),
                    gotime::format((engine.now)()),
                    "completed",
                    summary_json,
                    i64::try_from(run.archived_ids.len()).unwrap_or(i64::MAX),
                    i64::try_from(run.new_memory_ids.len()).unwrap_or(i64::MAX)
                ],
            )
            .map_err(StoreError::from)
        });
        if let Err(error) = saved {
            eprintln!("consolidation: failed to save run journal: {error}");
        }
    }

    /// `GetLastCompletedRun`: the newest run that has not been undone.
    ///
    /// # Errors
    /// A SQLite error.
    pub fn last_completed_consolidation_run(&self) -> Result<Option<ConsolidationRun>, StoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, run_at, status, summary_json, total_archived, total_consolidated FROM consolidation_runs WHERE status = 'completed' ORDER BY run_at DESC LIMIT 1",
            [],
            |row| {
                Ok(ConsolidationRun {
                    id: row.get(0)?,
                    run_at: row.get(1)?,
                    status: row.get(2)?,
                    summary_json: row.get(3)?,
                    total_archived: row.get(4)?,
                    total_consolidated: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// `UndoLastConsolidation`: deletes the run's consolidated memories,
    /// moves their evidence back, re-activates the archived originals as
    /// `raw` and marks the run undone, all in one transaction.
    ///
    /// # Errors
    /// `no completed consolidation run found to undo`, or a Go-worded
    /// SQLite/parse failure.
    pub fn undo_last_consolidation(&self) -> Result<String, StoreError> {
        let run = self
            .last_completed_consolidation_run()
            .map_err(|e| {
                StoreError::Invalid(format!("failed to query last consolidation run: {e}"))
            })?
            .ok_or_else(|| {
                StoreError::Invalid("no completed consolidation run found to undo".into())
            })?;
        let summary: RunSummary = serde_json::from_str(&run.summary_json)
            .map_err(|e| StoreError::Invalid(format!("failed to parse run summary: {e}")))?;
        let run_at = gotime::parse(&run.run_at).ok_or_else(|| {
            StoreError::Invalid(format!("parse run_at {:?}: unsupported layout", run.run_at))
        })?;
        let conn = self.lock()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| StoreError::Invalid(format!("failed to begin transaction: {e}")))?;
        let fail =
            |what: String, e: &dyn std::fmt::Display| StoreError::Invalid(format!("{what}: {e}"));
        for (old, new) in &summary.replaced_to_new {
            if old == new {
                continue;
            }
            crate::evidence_store::reparent_memory_evidence_tx(&tx, new, old).map_err(|e| {
                fail(
                    format!("failed to reparent evidence from {new} to {old}"),
                    &e,
                )
            })?;
        }
        // Go deleted the consolidated rows first; with cascading evidence
        // foreign keys that would drop the evidence before it is moved back,
        // so the rows are removed only after the reparenting.
        for id in &summary.new_memory_ids {
            tx.execute("DELETE FROM memories WHERE id = ?", [id])
                .map_err(|e| fail(format!("failed to delete consolidated memory {id}"), &e))?;
        }
        let now = chrono::Utc::now();
        for id in &summary.archived_ids {
            update_status(&tx, id, "raw", None, now)
                .map_err(|e| fail(format!("failed to re-activate memory {id}"), &e))?;
        }
        tx.execute(
            "UPDATE consolidation_runs SET status = 'undone' WHERE id = ?",
            [&run.id],
        )
        .map_err(|e| fail(format!("failed to mark run {} as undone", run.id), &e))?;
        tx.commit()
            .map_err(|e| fail("failed to commit undo transaction".into(), &e))?;
        Ok(format!(
            "Undid consolidation run {} (from {}):\n  Deleted consolidated memories: {}\n  Re-activated archived originals: {}\n  Reparented evidence pairs: {}",
            run.id.chars().take(8).collect::<String>(),
            run_at.format("%Y-%m-%d %H:%M:%S"),
            summary.new_memory_ids.len(),
            summary.archived_ids.len(),
            summary.replaced_to_new.len()
        ))
    }
}
