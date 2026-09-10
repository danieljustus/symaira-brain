//! Server aliases, mode presets, access classes, and exposure source constants.

pub const SERVER_VAULT: &str = "vault";
pub const SERVER_MEMORY: &str = "memory";
pub const SERVER_SKILLS: &str = "skills";
pub const SERVER_USAGE: &str = "usage";
pub const SERVER_OPERATE: &str = "operate";
pub const SERVER_SCOPE: &str = "scope";

/// Canonical aliases for all built-in and optional Brain modules.
pub const CORE_SERVER_ALIASES: [&str; 6] = [
    SERVER_VAULT,
    SERVER_MEMORY,
    SERVER_SKILLS,
    SERVER_USAGE,
    SERVER_OPERATE,
    SERVER_SCOPE,
];

#[must_use]
pub fn is_core_alias(alias: &str) -> bool {
    CORE_SERVER_ALIASES.contains(&alias)
}

pub const MEMORY_MODE_READ_ONLY: &str = "read_only";
pub const MEMORY_MODE_READ_WRITE: &str = "read_write";
pub const VAULT_MODE_REQUEST_ONLY: &str = "request_only";
pub const VAULT_MODE_FULL: &str = "full";
pub const VAULT_MODE_OFF: &str = "off";
pub const FOREIGN_ACCESS_READ: &str = "read";
pub const FOREIGN_ACCESS_WRITE: &str = "write";
pub const EXPOSURE_SOURCE_TOOLS_READ: &str = "tools_read";
pub const EXPOSURE_SOURCE_TOOLS_WRITE: &str = "tools_write";
pub const EXPOSURE_SOURCE_READ_ONLY_HINT: &str = "read_only_hint";
pub const EXPOSURE_SOURCE_DEFAULT_WRITE: &str = "default_write";
