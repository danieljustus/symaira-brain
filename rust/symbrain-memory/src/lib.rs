//! Embedded SQLite memory store used by the native MCP gateway.
#![deny(unsafe_code)]

mod activity;
mod embedding;
mod entity;
mod gojson;
mod gorand;
mod gotime;
mod list_rows;
mod lsh;
mod model;
mod retrieval;
mod rows;
mod schema;
mod search_rows;
mod store;

pub use activity::{ActivityItem, ActivityPage, ActivitySearch, ActivityStatus, Provenance};
pub use embedding::{EmbeddingGenerator, GeneratedEmbedding};
pub use list_rows::{MemoryListRow, RuleRow};
pub use model::{Memory, SetOptions, Store, StoreError};
pub use search_rows::{SearchHit, SearchRow};
