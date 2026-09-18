//! Embedded SQLite memory store used by the native MCP gateway.
#![deny(unsafe_code)]

mod activity;
mod embedding;
mod entity;
mod gojson;
mod gotime;
mod list_rows;
mod model;
mod rows;
mod schema;
mod store;

pub use activity::{ActivityItem, ActivityPage, ActivitySearch, ActivityStatus, Provenance};
pub use list_rows::MemoryListRow;
pub use model::{Memory, SetOptions, Store, StoreError};
