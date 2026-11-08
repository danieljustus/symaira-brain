//! Session importers (ported for #761 from `internal/memory/importer` at
//! v0.12.0).
//!
//! An importer discovers sessions of an external tool and turns each into
//! [`ImportedFact`]s. Every importer takes the location it reads from as a
//! constructor argument, so tests run on synthetic fixtures in a temp dir and
//! nothing here ever resolves a real home directory by itself. The
//! [`registry::Registry`] orchestrates incremental runs against the
//! `import_state` table.
// Go compares file extensions case-sensitively; the ports keep that.
// Request/prompt text is assembled with push_str(format!) for readability.
#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::format_push_string,
    clippy::struct_field_names
)]

pub mod calendar;
pub mod codexmemory;
pub mod curatedmemory;
mod distill;
pub mod email;
pub mod git;
pub mod github;
pub mod hermes;
pub mod http;
mod markdown;
pub mod memorytool;
pub mod obsidian;
pub mod opencode;
pub mod paperless;
pub mod registry;
pub mod runner;
pub mod shellhistory;
pub mod state;
mod store_row;
pub mod transcripts;
mod util;

#[cfg(test)]
mod registry_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub use util::{from_millis, modified_at, parse_object, parse_timestamp, walk_files};

use crate::evidence::Extraction;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// A discovered session of an external tool.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionRef {
    /// Tool name (`claude-code`, `codex`, `hermes`, ...).
    pub tool: String,
    /// Unique session identifier.
    pub session_id: String,
    /// File path or DB location.
    pub path: String,
    /// Last modification time.
    pub modified_at: DateTime<Utc>,
    /// Tool-specific metadata.
    pub metadata: BTreeMap<String, String>,
}

/// A memory fact extracted from a session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportedFact {
    /// The memory content.
    pub content: String,
    /// Tool name.
    pub source: String,
    /// Session identifier.
    pub session_id: String,
    /// When the fact was created.
    pub timestamp: DateTime<Utc>,
    /// Tool-specific context.
    pub metadata: BTreeMap<String, String>,
    /// Grounded spans backing `content`, if any.
    pub evidence: Vec<Extraction>,
}

/// Sensitivity of imported data, ascending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivacyLevel {
    /// Non-sensitive (e.g. git commits).
    Public,
    /// Internal tooling data.
    Internal,
    /// PII, business data.
    Confidential,
    /// Credentials, keys.
    Secret,
}

impl PrivacyLevel {
    /// The wire word stored in `sensitivity` metadata.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
            Self::Secret => "secret",
        }
    }
}

/// Result alias: importer failures are plain Go-worded messages.
pub type ImportResult<T> = Result<T, String>;

/// An external-tool importer. The optional Go interfaces
/// (`Categorizable`, `PrivacyAware`, `IncrementalImporter`,
/// `TranscriptImporter`, `StagedImporter`, `UntrustedContentImporter`) are
/// default methods.
pub trait SessionImporter {
    /// Tool name.
    fn name(&self) -> &str;
    /// Sessions modified since `since`.
    ///
    /// # Errors
    /// A message describing why discovery failed.
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>>;
    /// Facts of one session.
    ///
    /// # Errors
    /// A message describing why the session could not be read.
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>>;
    /// Category (`code`, `communication`, `calendar`, `documents`, `notes`, `activity`).
    fn category(&self) -> Option<&str> {
        None
    }
    /// Declared privacy level (`PrivacyAware`).
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        None
    }
    /// The importer's own cursor (`IncrementalImporter.LastImportTime`);
    /// `None` when the importer keeps no cursor.
    ///
    /// # Errors
    /// A message when the cursor cannot be read.
    fn last_import_time(&self) -> ImportResult<Option<DateTime<Utc>>> {
        Ok(None)
    }
    /// Advances the importer's own cursor.
    ///
    /// # Errors
    /// A message when the cursor cannot be written.
    fn mark_imported(&self, _session: &SessionRef) -> ImportResult<()> {
        Ok(())
    }
    /// Raw conversation transcripts that extraction should distill.
    fn is_transcript(&self) -> bool {
        false
    }
    /// Facts enter the review queue instead of becoming live memories.
    fn stage_imported_facts(&self) -> bool {
        false
    }
    /// Content may contain instruction-like text and is sanitized.
    fn content_is_untrusted(&self) -> bool {
        false
    }
}
