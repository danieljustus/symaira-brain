//! Configuration inspection and mutation matching the Go CLI contract.

mod atomic;
mod encode;
mod format;
mod get;
mod lossless;
mod set;

pub use format::format_go_quoted;
pub use get::{run_config_get, run_config_get_with_path};
pub use set::{run_config_set, run_config_set_with_path};
