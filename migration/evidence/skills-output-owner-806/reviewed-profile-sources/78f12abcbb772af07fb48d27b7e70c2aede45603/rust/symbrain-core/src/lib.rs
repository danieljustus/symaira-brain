#![deny(unsafe_code)]

pub mod config;
pub mod exit;
pub mod go_path;
mod go_text;
mod json_decode;
mod json_string;
pub mod output;
pub mod paths;
pub mod version;
pub mod xdg;

pub use output::{OutputError, OutputFormat};
pub use paths::Location;
pub use version::VersionInfo;

pub use go_text::GoText;
pub use json_decode::go_json_compatible_text;
pub use json_string::go_json_string_bytes;
