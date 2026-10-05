//! Embedded SQLite memory store used by the native MCP gateway.
#![deny(unsafe_code)]

mod activity;
mod cli_delete_admission;
mod cli_entity;
mod cli_write;
mod direct_admission;
mod embedding;
mod entity;
pub mod evidence;
mod evidence_store;
mod gojson;
mod gorand;
mod gotime;
mod list_rows;
mod lsh;
mod migration;
mod model;
mod retrieval;
mod rows;
mod schema;
mod search_rows;
mod store;
mod write;

#[cfg(test)]
#[path = "db_oracle_tests.rs"]
mod db_oracle_tests;

pub use activity::{ActivityItem, ActivityPage, ActivitySearch, ActivityStatus, Provenance};
pub use cli_delete_admission::direct_delete_supported;
pub use cli_entity::direct_entities_supported;
pub use cli_write::{DirectWrite, direct_project_supported};
pub use direct_admission::{direct_content_supported, direct_text_supported};
pub use embedding::{EmbeddingGenerator, GeneratedEmbedding};
pub use list_rows::{MemoryListRow, RuleRow};
pub use model::{Memory, SetOptions, Store, StoreError};
pub use search_rows::{SearchHit, SearchRow};

pub use evidence_store::{EvidenceSpan, reparent_memory_evidence_tx, save_memory_evidence_tx};
