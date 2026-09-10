//! Server aliases, mode presets, access classes, and exposure source constants.

/// Core server alias for Symaira Vault.
pub const SERVER_VAULT: &str = "vault";

/// Core server alias for Symaira Memory.
pub const SERVER_MEMORY: &str = "memory";

/// Core server alias for Symaira Skills.
pub const SERVER_SKILLS: &str = "skills";

/// Core server alias for Symaira Usage.
pub const SERVER_USAGE: &str = "usage";

/// Canonical core server aliases in display and resolution order.
pub const CORE_SERVER_ALIASES: [&str; 4] =
    [SERVER_VAULT, SERVER_MEMORY, SERVER_SKILLS, SERVER_USAGE];

/// Reports whether `alias` is one of the four reserved core server aliases.
#[must_use]
pub fn is_core_alias(alias: &str) -> bool {
    CORE_SERVER_ALIASES.contains(&alias)
}

/// Memory server read-only mode preset.
pub const MEMORY_MODE_READ_ONLY: &str = "read_only";

/// Memory server read-write mode preset.
pub const MEMORY_MODE_READ_WRITE: &str = "read_write";

/// Vault server request-only mode preset.
pub const VAULT_MODE_REQUEST_ONLY: &str = "request_only";

/// Vault server full-access mode preset.
pub const VAULT_MODE_FULL: &str = "full";

/// Vault server off (disabled exposure) mode preset.
pub const VAULT_MODE_OFF: &str = "off";

/// Foreign server read-only exposure access class.
pub const FOREIGN_ACCESS_READ: &str = "read";

/// Foreign server read-write exposure access class (default).
pub const FOREIGN_ACCESS_WRITE: &str = "write";

/// Tool exposure classification source: explicitly listed in `tools_read`.
pub const EXPOSURE_SOURCE_TOOLS_READ: &str = "tools_read";

/// Tool exposure classification source: explicitly listed in `tools_write`.
pub const EXPOSURE_SOURCE_TOOLS_WRITE: &str = "tools_write";

/// Tool exposure classification source: upstream tool `readOnlyHint` annotation.
pub const EXPOSURE_SOURCE_READ_ONLY_HINT: &str = "read_only_hint";

/// Tool exposure classification source: default write class for unannotated foreign tools.
pub const EXPOSURE_SOURCE_DEFAULT_WRITE: &str = "default_write";
