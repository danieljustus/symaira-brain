//! Read-only source adapters for the existing memory importer domain.
//!
//! This checkpoint adds library contracts only. It neither registers a CLI
//! command nor bypasses the registry's sanitization, redaction and staging.
mod aider;
mod bytes;
mod codex_discovery;
mod codex_memory;
mod curated;
mod files;
mod markdown;
mod model;
mod obsidian;
mod shell;
mod state;

pub use aider::AiderImporter;
pub use codex_memory::{ApplicationPolicy, CodexMemoryImporter};
pub use curated::CuratedMemoryImporter;
pub use model::{
    Batch, Fact, GroundedFact, ImportError, Metadata, SessionImporter, SessionRef, Traits,
};
pub use obsidian::ObsidianImporter;
pub use shell::ShellHistoryImporter;

#[cfg(test)]
mod resource_time_tests;
#[cfg(test)]
mod tests;
