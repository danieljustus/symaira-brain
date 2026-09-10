//! Pure profile configuration and policy evaluation engine for Symaira Brain.
//!
//! Provides the foundational domain logic for:
//! - Parsing, validating, and defaulting profile TOML files.
//! - Server exposure presets and allow/deny lists.
//! - Default-deny gatekeeping for closed-universe core servers (`vault`, `memory`, `usage`, `skills`).
//! - Read/write filtering and annotation-based classification for open foreign MCP servers.

#![deny(unsafe_code)]

pub mod constants;
pub mod error;
pub mod policy;
pub mod profile;

pub use constants::*;
pub use error::{PolicyError, ProfileError};
pub use policy::{
    ForeignTool, Report, ToolAnnotations, ToolExposure, Verdict, evaluate, evaluate_foreign,
    evaluate_preset, identity_parameter, known_tools, preset_tools,
};
pub use profile::{
    AuditConfig, LoadResult, Profile, ServerConfig, Servers, exists, list_names, load, load_all,
    load_file, path, validate_name,
};
