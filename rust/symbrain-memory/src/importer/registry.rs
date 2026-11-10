//! The importer registry (ported from `internal/memory/importer/registry.go`).
//!
//! `run_import` discovers sessions since the last import (30 days back on a
//! first run), skips sessions already in `import_state`, optionally sanitizes
//! untrusted content and distills transcripts, then stores the facts as raw
//! `agent`-scope memories with provenance/trust/sharing metadata, redacting
//! PII, and marks the session imported. Collaborators (embedder, clock, ids)
//! are injected.

use super::distill::{extract_facts, sanitize_facts};
use super::store_row::{AgentRow, insert_agent_memory};
use super::{ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::consolidation::EmbedFn;
use crate::model::{Store, StoreError};
use crate::pii::{redact, redact_map};
use crate::write;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;

/// Outcome of one tool's run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolResult {
    /// Tool name.
    pub tool: String,
    /// Sessions imported (or that would be, on a dry run).
    pub sessions: usize,
    /// Facts produced.
    pub facts: usize,
    /// Sessions skipped as already imported.
    pub skipped: usize,
    /// Errors met.
    pub errors: usize,
    /// Dry run flag.
    pub dry_run: bool,
}

/// Orchestrates importers against a store.
pub struct Registry<'a> {
    store: &'a Store,
    importers: BTreeMap<String, Box<dyn SessionImporter + 'a>>,
    extract_on_import: bool,
    embed: EmbedFn<'a>,
    now: &'a dyn Fn() -> DateTime<Utc>,
    new_id: &'a dyn Fn() -> String,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> Registry<'a> {
    /// New registry with the production clock/ids and the hash embedder.
    #[must_use]
    pub fn new(store: &'a Store, extract_on_import: bool) -> Self {
        Self {
            store,
            importers: BTreeMap::new(),
            extract_on_import,
            embed: &crate::consolidation::hash_embed,
            now: &Utc::now,
            new_id: &write::new_uuid,
            cancelled: &|| false,
        }
    }

    /// Injects embedder, clock, id source and cancellation probe.
    #[must_use]
    pub fn with(
        mut self,
        embed: EmbedFn<'a>,
        now: &'a dyn Fn() -> DateTime<Utc>,
        new_id: &'a dyn Fn() -> String,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        self.embed = embed;
        self.now = now;
        self.new_id = new_id;
        self.cancelled = cancelled;
        self
    }

    /// Registers (or replaces) an importer by name.
    pub fn register(&mut self, importer: impl SessionImporter + 'a) {
        self.importers
            .insert(importer.name().to_owned(), Box::new(importer));
    }

    /// Looks an importer up.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&(dyn SessionImporter + 'a)> {
        self.importers.get(name).map(Box::as_ref)
    }

    /// Registered names, sorted.
    #[must_use]
    pub fn list(&self) -> Vec<String> {
        self.importers.keys().cloned().collect()
    }

    /// Names of importers in `category`.
    #[must_use]
    pub fn list_by_category(&self, category: &str) -> Vec<String> {
        self.importers
            .values()
            .filter(|i| i.category() == Some(category))
            .map(|i| i.name().to_owned())
            .collect()
    }

    /// Names whose privacy level is at or below `max`; importers that declare
    /// none always qualify.
    #[must_use]
    pub fn list_by_privacy_level(&self, max: PrivacyLevel) -> Vec<String> {
        self.importers
            .values()
            .filter(|i| i.privacy_level().is_none_or(|level| level <= max))
            .map(|i| i.name().to_owned())
            .collect()
    }

    /// `RunImport` for `tools` (all when empty).
    ///
    /// # Errors
    /// `unknown importer ...` before anything runs for that tool; per-tool
    /// failures become an error row, as in Go.
    pub fn run_import(&self, tools: &[String], dry_run: bool) -> Result<Vec<ToolResult>, String> {
        let tools = if tools.is_empty() {
            self.list()
        } else {
            tools.to_vec()
        };
        let mut results = Vec::new();
        for name in tools {
            let Some(importer) = self.importers.get(&name) else {
                return Err(format!(
                    "unknown importer {name:?}; valid importers: {}",
                    self.list().join(", ")
                ));
            };
            match self.run_tool(importer.as_ref(), dry_run) {
                Ok(result) => results.push(result),
                Err(error) => {
                    eprintln!("Error importing from {name}: {error}");
                    results.push(ToolResult {
                        tool: name,
                        errors: 1,
                        dry_run,
                        ..ToolResult::default()
                    });
                }
            }
        }
        Ok(results)
    }

    fn run_tool(
        &self,
        importer: &dyn SessionImporter,
        dry_run: bool,
    ) -> Result<ToolResult, String> {
        let name = importer.name();
        let mut result = ToolResult {
            tool: name.to_owned(),
            dry_run,
            ..ToolResult::default()
        };
        let mut last = self
            .store
            .last_import_time(name)
            .map_err(|e| format!("failed to get last import time: {e}"))?;
        if let Some(own) = importer
            .last_import_time()
            .map_err(|e| format!("failed to get importer cursor: {e}"))?
            && last.is_none_or(|stored| own > stored)
        {
            last = Some(own);
        }
        let since = last.unwrap_or_else(|| (self.now)() - Duration::days(30));
        let sessions = importer
            .discover_sessions(since)
            .map_err(|e| format!("failed to discover sessions: {e}"))?;
        eprintln!("Found {} sessions from {name}", sessions.len());
        let total = sessions.len();
        for (index, session) in sessions.iter().enumerate() {
            // Diagnostics name the session by position, never by its id or path.
            let label = format!("session {}/{total} of {name}", index + 1);
            if (self.cancelled)() {
                return Err("context canceled".into());
            }
            match self.store.is_session_imported(name, &session.session_id) {
                Err(e) => {
                    eprintln!("Error checking import state for {label}: {e}");
                    result.errors += 1;
                    continue;
                }
                Ok(true) => {
                    result.skipped += 1;
                    continue;
                }
                Ok(false) => {}
            }
            let mut facts = match importer.import_session(session) {
                Ok(facts) => facts,
                Err(e) => {
                    eprintln!("Error importing {label}: {e}");
                    result.errors += 1;
                    continue;
                }
            };
            if importer.content_is_untrusted() {
                sanitize_facts(&mut facts);
            }
            if importer.is_transcript() {
                if self.extract_on_import {
                    facts = extract_facts(facts);
                } else {
                    eprintln!("Extraction disabled for {name}; storing raw facts");
                }
            }
            result.sessions += 1;
            result.facts += facts.len();
            if !dry_run {
                self.finish_session(importer, session, &label, &facts, &mut result);
            }
        }
        Ok(result)
    }

    fn finish_session(
        &self,
        importer: &dyn SessionImporter,
        session: &SessionRef,
        label: &str,
        facts: &[ImportedFact],
        result: &mut ToolResult,
    ) {
        if let Err(e) = self.store_facts(facts, importer) {
            eprintln!("Error storing facts for {label}: {e}");
            result.errors += 1;
            return;
        }
        if let Err(e) = self.store.mark_session_imported(
            importer.name(),
            &session.session_id,
            facts.len(),
            (self.now)(),
        ) {
            eprintln!("Error marking {label} as imported: {e}");
            result.errors += 1;
        }
        if let Err(e) = importer.mark_imported(session) {
            eprintln!("Error marking importer cursor for {label}: {e}");
            result.errors += 1;
        }
    }

    fn store_facts(
        &self,
        facts: &[ImportedFact],
        importer: &dyn SessionImporter,
    ) -> Result<(), StoreError> {
        for fact in facts {
            let now = (self.now)();
            let mut metadata = fact.metadata.clone();
            let source_uri = metadata.get("session_id").cloned().unwrap_or_default();
            for (key, value) in [
                ("source_type", "imported".to_owned()),
                ("source_tool", fact.source.clone()),
                ("source_uri", source_uri),
                (
                    "observed_at",
                    fact.timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                ),
            ] {
                metadata.insert(key.into(), value);
            }
            if let Some(level) = importer.privacy_level() {
                metadata.insert("sensitivity".into(), level.as_str().into());
            }
            for (key, value) in [
                ("authority", "inferred"),
                ("confidence", "medium"),
                ("verification_status", "unverified"),
            ] {
                metadata.insert(key.into(), value.into());
            }
            metadata
                .entry("sharing_level".into())
                .or_insert_with(|| "private".into());
            let content = redact(&fact.content);
            let (vector, source, model) = (self.embed)(&content);
            let id = (self.new_id)();
            let staged = importer.stage_imported_facts();
            insert_agent_memory(
                self.store,
                &AgentRow {
                    id: &id,
                    content: &content,
                    content_hash: &write::content_hash(&content),
                    metadata: &redact_map(&metadata),
                    created: fact.timestamp,
                    now,
                    staged,
                    embedding: (&vector, &source, &model),
                },
            )?;
            if !fact.evidence.is_empty() {
                let redacted: Vec<_> = fact
                    .evidence
                    .iter()
                    .cloned()
                    .map(|mut e| {
                        e.evidence_text = redact(&e.evidence_text);
                        e.text = redact(&e.text);
                        e
                    })
                    .collect();
                self.store
                    .save_memory_evidence(&id, &redacted)
                    .map_err(|e| {
                        StoreError::Invalid(format!("failed to save memory evidence: {e}"))
                    })?;
            }
        }
        Ok(())
    }
}
