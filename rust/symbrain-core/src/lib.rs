#![deny(unsafe_code)]

pub mod config;
pub mod exit;
pub mod go_json_float;
pub mod output;
pub mod paths;
pub mod version;
pub mod xdg;

pub use output::{OutputError, OutputFormat};
pub use paths::Location;
pub use version::VersionInfo;
