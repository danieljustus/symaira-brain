//! Embedded SQLite memory store used by the native MCP gateway.
#![deny(unsafe_code)]

mod activity;
mod entity;
mod model;
mod rows;
mod schema;
mod store;

pub use activity::{ActivityItem, ActivityPage, ActivitySearch, ActivityStatus, Provenance};
pub use model::{Memory, SetOptions, Store, StoreError};
