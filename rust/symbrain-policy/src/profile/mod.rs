//! Profile schema, loading, defaults, and validation.

pub mod create;
pub mod fs;
pub mod parse;
pub mod resolve;
pub mod undecoded;
pub mod validate;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use fs::{LoadResult, exists, list_names, load, load_all, load_file, path};
pub use validate::validate_name;

/// Mapping of server aliases to their exposure configurations.
pub type Servers = BTreeMap<String, ServerConfig>;

/// Parsed, validated, and defaulted representation of a profile file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Safe profile name matching the filename basename.
    pub name: String,
    /// Human-readable description of this profile.
    pub description: String,
    /// Server exposure configuration per alias.
    pub servers: Servers,
    /// Global audit configuration for connections under this profile.
    pub audit: AuditConfig,
    /// Non-fatal warnings found during parsing or validation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl Profile {
    /// Returns the [`ServerConfig`] for `alias`, or a default (disabled) config if absent.
    #[must_use]
    pub fn server(&self, alias: &str) -> ServerConfig {
        self.servers.get(alias).cloned().unwrap_or_default()
    }

    /// Returns server aliases in canonical order:
    /// the four cores (`vault`, `memory`, `skills`, `usage`) followed by foreign servers alphabetically.
    #[must_use]
    pub fn server_aliases(&self) -> Vec<String> {
        let mut aliases = Vec::with_capacity(self.servers.len());
        for core in crate::constants::CORE_SERVER_ALIASES {
            if self.servers.contains_key(core) {
                aliases.push(core.to_string());
            }
        }
        let mut foreign: Vec<String> = self
            .servers
            .keys()
            .filter(|k| !crate::constants::is_core_alias(k))
            .cloned()
            .collect();
        foreign.sort();
        aliases.extend(foreign);
        aliases
    }
}

/// Configuration for one server alias in a profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Whether the server is enabled.
    pub enabled: bool,
    /// Mode preset (meaningful for vault and memory; ignored for skills/usage/foreign).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// Explicit tool allowlist that overrides preset defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_allow: Vec<String>,
    /// Explicit tool denylist that unconditionally removes tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_deny: Vec<String>,
    /// Command transport for a foreign server.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Command arguments for a foreign server.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// URL transport for a foreign server.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Exposure access class for foreign servers ("read" or "write").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access: String,
    /// Explicit read classification overrides for foreign server tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_read: Vec<String>,
    /// Explicit write classification overrides for foreign server tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_write: Vec<String>,
}

/// Audit configuration for a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Whether audit logging is enabled.
    pub enabled: bool,
    /// Whether non-sensitive argument values are included in audit entries.
    #[serde(skip_serializing_if = "is_false")]
    pub verbose: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            verbose: false,
        }
    }
}
