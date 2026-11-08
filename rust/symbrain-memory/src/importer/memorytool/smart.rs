//! De-duplicating store of imported memory-tool facts (`smartimport.go`).

use super::{ImportSummary, ToolFact};
use crate::importer::store_row::{AgentRow, insert_agent_memory};
use crate::model::{Store, StoreError};
use crate::pii::{redact, redact_map};
use crate::write;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// Stores facts as raw `agent` memories unless their content hash is known.
pub struct SmartImporter<'a> {
    store: &'a Store,
    now: fn() -> DateTime<Utc>,
}

impl<'a> SmartImporter<'a> {
    /// A smart importer over `store`.
    #[must_use]
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            now: Utc::now,
        }
    }

    /// Replaces the clock (tests).
    #[must_use]
    pub fn with_clock(mut self, now: fn() -> DateTime<Utc>) -> Self {
        self.now = now;
        self
    }

    /// Imports `facts`; `dry_run` counts without writing.
    ///
    /// # Errors
    /// The existence check or the insert failed.
    pub fn import_facts(
        &self,
        facts: &[ToolFact],
        dry_run: bool,
    ) -> Result<ImportSummary, StoreError> {
        let mut summary = ImportSummary::default();
        for fact in facts {
            // The hash covers the original text; the stored text is redacted.
            let hash = write::content_hash(&fact.content);
            let exists = self
                .fact_exists(&hash)
                .map_err(|e| StoreError::Invalid(format!("failed to check fact existence: {e}")))?;
            if exists {
                summary.skipped += 1;
                continue;
            }
            if !dry_run {
                self.store_fact(fact, &hash)
                    .map_err(|e| StoreError::Invalid(format!("failed to store fact: {e}")))?;
            }
            summary.created += 1;
        }
        Ok(summary)
    }

    fn fact_exists(&self, hash: &str) -> Result<bool, StoreError> {
        let conn = self.store.lock()?;
        let mut statement = conn
            .prepare("SELECT 1 FROM memories WHERE content_hash = ? LIMIT 1")
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        statement
            .exists([hash])
            .map_err(|e| StoreError::Invalid(e.to_string()))
    }

    fn store_fact(&self, fact: &ToolFact, hash: &str) -> Result<(), StoreError> {
        let metadata: BTreeMap<String, String> = fact
            .metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.sprint()))
            .collect();
        insert_agent_memory(
            self.store,
            &AgentRow {
                id: &write::new_uuid(),
                content: &redact(&fact.content),
                content_hash: hash,
                metadata: &redact_map(&metadata),
                created: fact.timestamp,
                now: (self.now)(),
                staged: false,
                embedding: (&[], "", ""),
            },
        )
    }
}
