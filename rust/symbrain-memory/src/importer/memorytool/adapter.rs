//! Registers a [`MemoryToolImporter`] as a [`SessionImporter`]
//! (`adapter.go`): every export file is one session.

use super::{ExportRef, MemoryToolImporter};
use crate::importer::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, Utc};

/// Wraps a memory-tool importer for the session registry.
pub struct SessionAdapter {
    importer: Box<dyn MemoryToolImporter>,
    path: String,
}

impl SessionAdapter {
    /// An adapter over `importer`; an empty `path` selects its default location.
    #[must_use]
    pub fn new(importer: Box<dyn MemoryToolImporter>, path: &str) -> Self {
        Self {
            importer,
            path: path.to_owned(),
        }
    }

    /// Memory-tool exports may contain PII (`RequiresPIIGuard`).
    #[must_use]
    pub const fn requires_pii_guard(&self) -> bool {
        true
    }
}

impl SessionImporter for SessionAdapter {
    fn name(&self) -> &str {
        self.importer.name()
    }

    fn category(&self) -> Option<&str> {
        Some("memory-tool")
    }

    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }

    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        Ok(self
            .importer
            .discover_exports(&self.path)?
            .into_iter()
            .filter(|export| export.modified_at >= since)
            .map(|export| SessionRef {
                tool: export.tool,
                session_id: export.path.clone(),
                path: export.path,
                modified_at: export.modified_at,
                ..SessionRef::default()
            })
            .collect())
    }

    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let export = ExportRef {
            tool: session.tool.clone(),
            path: session.path.clone(),
            format: "json".into(),
            modified_at: session.modified_at,
        };
        Ok(self
            .importer
            .import_export(&export)?
            .into_iter()
            .map(|fact| ImportedFact {
                content: fact.content,
                source: fact.source,
                session_id: session.session_id.clone(),
                timestamp: fact.timestamp,
                metadata: fact
                    .metadata
                    .iter()
                    .map(|(key, value)| (key.clone(), value.sprint()))
                    .collect(),
                evidence: Vec::new(),
            })
            .collect())
    }
}
