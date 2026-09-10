//! In-repo maintained tool universes and mode presets.

use crate::constants::{
    MEMORY_MODE_READ_ONLY, MEMORY_MODE_READ_WRITE, SERVER_MEMORY, SERVER_OPERATE, SERVER_SCOPE,
    SERVER_USAGE, SERVER_VAULT, VAULT_MODE_FULL, VAULT_MODE_OFF, VAULT_MODE_REQUEST_ONLY,
};
use crate::error::PolicyError;

/// Vault tools exposed under the `request_only` preset.
pub const VAULT_TOOLS_REQUEST_ONLY: &[&str] =
    &["request_credential", "generate_password", "health"];

/// Vault tools exposed under the `full` preset.
pub const VAULT_TOOLS_FULL: &[&str] = &[
    "find_entries",
    "generate_password",
    "get_entry",
    "get_entry_metadata",
    "health",
    "request_credential",
    "set_entry_field",
    "symaira_audit_self",
    "symaira_search",
    "symaira_whoami",
];

/// Memory tools exposed under the `read_only` preset.
pub const MEMORY_TOOLS_READ_ONLY: &[&str] = &[
    "entity_list",
    "entity_resolve",
    "graph_neighbors",
    "memory_get",
    "memory_list",
    "memory_search",
];

/// Memory tools exposed under the `read_write` preset.
pub const MEMORY_TOOLS_READ_WRITE: &[&str] = &[
    "entity_list",
    "entity_relate",
    "entity_resolve",
    "graph_neighbors",
    "memory_get",
    "memory_list",
    "memory_search",
    "memory_set",
];

/// Usage tools exposed by the usage server.
pub const USAGE_TOOLS: &[&str] = &["get_ai_usage"];

/// Activity tools that require explicit profile allowlists (not part of default presets).
pub const OPERATE_TOOLS: &[&str] = &["version", "permissions_status", "get_policy"];
pub const SCOPE_TOOLS: &[&str] = &[
    "scan",
    "ports_list",
    "ports_suggest",
    "mcp_list",
    "mcp_health",
    "daemons_list",
    "conflicts",
];

pub const ACTIVITY_TOOLS: &[&str] = &["activity_get", "activity_search", "activity_status"];

/// Returns the maximal versioned tool universe this crate knows for `alias`.
#[must_use]
pub fn universe_for(alias: &str) -> Option<&'static [&'static str]> {
    match alias {
        SERVER_VAULT => Some(VAULT_TOOLS_FULL),
        SERVER_MEMORY => Some(MEMORY_TOOLS_READ_WRITE),
        SERVER_USAGE => Some(USAGE_TOOLS),
        SERVER_OPERATE => Some(OPERATE_TOOLS),
        SERVER_SCOPE => Some(SCOPE_TOOLS),
        _ => None,
    }
}

/// Returns a copy of the maximal versioned tool list for `alias`.
///
/// Returns an empty vector for `skills`, which has no bounded preset universe.
#[must_use]
pub fn known_tools(alias: &str) -> Vec<String> {
    universe_for(alias)
        .map(|tools| tools.iter().map(|&s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Returns the static preset slice for `alias` and `mode` if recognized.
#[must_use]
pub fn preset_for_mode(alias: &str, mode: &str) -> Option<&'static [&'static str]> {
    match alias {
        SERVER_VAULT => match mode {
            VAULT_MODE_REQUEST_ONLY => Some(VAULT_TOOLS_REQUEST_ONLY),
            VAULT_MODE_FULL => Some(VAULT_TOOLS_FULL),
            VAULT_MODE_OFF => Some(&[]),
            _ => None,
        },
        SERVER_MEMORY => match mode {
            MEMORY_MODE_READ_ONLY => Some(MEMORY_TOOLS_READ_ONLY),
            MEMORY_MODE_READ_WRITE => Some(MEMORY_TOOLS_READ_WRITE),
            _ => None,
        },
        _ => None,
    }
}

/// Returns the versioned tool list for a given server and mode combination.
///
/// # Errors
///
/// Returns [`PolicyError::NoPreset`] if the combination is unrecognized.
pub fn preset_tools(alias: &str, mode: &str) -> Result<Vec<String>, PolicyError> {
    preset_for_mode(alias, mode)
        .map(|tools| tools.iter().map(|&s| s.to_string()).collect())
        .ok_or_else(|| PolicyError::NoPreset {
            server: alias.to_string(),
            mode: mode.to_string(),
        })
}
