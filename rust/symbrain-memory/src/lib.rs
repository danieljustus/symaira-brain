//! Embedded SQLite memory store used by the native MCP gateway.
#![deny(unsafe_code)]

mod activity;
mod cli_entity;
mod cli_write;
mod conflict;
mod conflict_llm;
mod embedding;
mod entity;
pub mod evidence;
mod evidence_store;
mod extract;
mod goduration;
mod gojson;
pub mod gojson_decode;
mod gorand;
mod goscan;
mod gotext;
mod gotime;
pub mod http;
mod list_rows;
mod lsh;
mod migration;
mod model;
pub mod pii;
mod relay_crypto;
mod retrieval;
mod rows;
mod schema;
mod schema_inspection;
mod search_rows;
mod serve_store;
mod set_pipeline;
mod slog;
mod store;
mod store_dir;
mod sync_apply;
pub mod sync_client;
mod sync_client_json;
mod sync_client_scan;
mod sync_client_store;
mod sync_client_time;
mod sync_state;
mod sync_time;
mod wal_open;
mod write;

#[cfg(test)]
#[path = "db_oracle_tests.rs"]
mod db_oracle_tests;

pub use activity::{ActivityItem, ActivityPage, ActivitySearch, ActivityStatus, Provenance};
pub use conflict::ConflictConfig;
pub use conflict_llm::LlmSettings;
pub use embedding::{EmbeddingGenerator, GeneratedEmbedding};
pub use goscan::raw_bytes;
pub use list_rows::{MemoryListRow, RuleRow};
pub use model::{Memory, SetOptions, Store, StoreError};
pub use retrieval::{PolicyFilter, SearchFilters, ServiceSearch, TrustFilter};
pub use schema_inspection::missing_required_columns;
pub use search_rows::{SearchHit, SearchRow};
pub use set_pipeline::{CliSet, Origin};
pub use slog::LogSink;
pub use sync_apply::{PullApplied, PulledDelete, PulledMemory, PulledPage};
pub use sync_state::{StoredCursor, parse_cursor, render_cursor, zero_time};

pub use evidence_store::{EvidenceSpan, reparent_memory_evidence_tx, save_memory_evidence_tx};
