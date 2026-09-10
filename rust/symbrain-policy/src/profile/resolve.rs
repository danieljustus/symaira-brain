//! Resolution of server configs with preset modes, foreign server validation, and defaults.

use crate::constants::{
    FOREIGN_ACCESS_READ, FOREIGN_ACCESS_WRITE, MEMORY_MODE_READ_ONLY, MEMORY_MODE_READ_WRITE,
    SERVER_MEMORY, SERVER_OPERATE, SERVER_SCOPE, SERVER_SKILLS, SERVER_USAGE, SERVER_VAULT,
    VAULT_MODE_FULL, VAULT_MODE_OFF, VAULT_MODE_REQUEST_ONLY, is_core_alias,
};
use crate::error::ProfileError;
use crate::profile::{ServerConfig, Servers};
use std::collections::BTreeMap;

/// Intermediate representation of a decoded server block before validation and defaulting.
#[derive(Debug, Clone, Default)]
pub struct RawServer {
    /// Explicitly parsed `enabled` boolean.
    pub enabled: Option<bool>,
    /// Mode string.
    pub mode: String,
    /// Explicit tools allow list.
    pub tools_allow: Vec<String>,
    /// Explicit tools deny list.
    pub tools_deny: Vec<String>,
    /// Foreign server command transport.
    pub command: String,
    /// Foreign server command arguments.
    pub args: Vec<String>,
    /// Foreign server URL transport.
    pub url: String,
    /// Foreign server access class ("read" or "write").
    pub access: String,
    /// Foreign server tools read classification overrides.
    pub tools_read: Vec<String>,
    /// Foreign server tools write classification overrides.
    pub tools_write: Vec<String>,
}

/// Resolves raw server tables into finalized [`ServerConfig`]s, enforcing core invariants
/// and validating foreign server declarations.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidServer`] if a core server carries foreign transport fields,
/// a foreign server lacks a transport or has an invalid access class, or a core server has an invalid mode.
pub fn resolve_servers(
    raw: &BTreeMap<String, RawServer>,
) -> Result<(Servers, Vec<String>), ProfileError> {
    let mut warnings = Vec::new();

    // Check that core aliases are not redefined as foreign servers.
    for (alias, fs) in raw {
        if !is_core_alias(alias) {
            continue;
        }
        if !fs.command.is_empty() || !fs.args.is_empty() || !fs.url.is_empty() {
            return Err(ProfileError::InvalidServer {
                name: alias.clone(),
                message: format!(
                    "servers.{alias}: core server cannot carry command/args/url — it is not a foreign server"
                ),
            });
        }
        if !fs.access.is_empty() || !fs.tools_read.is_empty() || !fs.tools_write.is_empty() {
            warnings.push(format!(
                "servers.{alias}: access/tools_read/tools_write are ignored (core exposure is governed by the mode preset)"
            ));
        }
    }

    let mut servers = Servers::new();

    // The four core aliases are always present, resolved with mode presets and default-deny.
    for alias in crate::constants::CORE_SERVER_ALIASES {
        let default_raw = RawServer::default();
        let fs = raw.get(alias).unwrap_or(&default_raw);
        match alias {
            SERVER_VAULT => {
                let sc = resolve_vault(fs)?;
                servers.insert(alias.to_string(), sc);
            }
            SERVER_MEMORY => {
                let sc = resolve_memory(fs)?;
                servers.insert(alias.to_string(), sc);
            }
            SERVER_SKILLS => {
                let (sc, sw) = resolve_skills(fs);
                warnings.extend(sw);
                servers.insert(alias.to_string(), sc);
            }
            SERVER_USAGE => {
                let (sc, uw) = resolve_usage(fs);
                warnings.extend(uw);
                servers.insert(alias.to_string(), sc);
            }
            SERVER_OPERATE | SERVER_SCOPE => {
                servers.insert(alias.to_string(), resolve_optional(fs));
            }
            _ => unreachable!(),
        }
    }

    // Foreign servers: sorted alphabetically.
    let mut foreign: Vec<&String> = raw.keys().filter(|k| !is_core_alias(k)).collect();
    foreign.sort();

    for alias in foreign {
        let fs = &raw[alias];
        let (sc, fw) = resolve_foreign(alias, fs)?;
        warnings.extend(fw);
        servers.insert(alias.clone(), sc);
    }

    Ok((servers, warnings))
}

fn resolve_vault(fs: &RawServer) -> Result<ServerConfig, ProfileError> {
    let mode = if fs.mode.is_empty() {
        VAULT_MODE_REQUEST_ONLY.to_string()
    } else {
        fs.mode.clone()
    };

    match mode.as_str() {
        VAULT_MODE_REQUEST_ONLY | VAULT_MODE_FULL | VAULT_MODE_OFF => Ok(ServerConfig {
            enabled: fs.enabled.unwrap_or(false),
            mode,
            tools_allow: fs.tools_allow.clone(),
            tools_deny: fs.tools_deny.clone(),
            ..ServerConfig::default()
        }),
        _ => Err(ProfileError::InvalidServer {
            name: SERVER_VAULT.to_string(),
            message: format!(
                "servers.vault: invalid mode \"{mode}\" (must be one of {VAULT_MODE_REQUEST_ONLY}, {VAULT_MODE_FULL}, {VAULT_MODE_OFF})"
            ),
        }),
    }
}

fn resolve_memory(fs: &RawServer) -> Result<ServerConfig, ProfileError> {
    let mode = if fs.mode.is_empty() {
        MEMORY_MODE_READ_ONLY.to_string()
    } else {
        fs.mode.clone()
    };

    match mode.as_str() {
        MEMORY_MODE_READ_ONLY | MEMORY_MODE_READ_WRITE => Ok(ServerConfig {
            enabled: fs.enabled.unwrap_or(false),
            mode,
            tools_allow: fs.tools_allow.clone(),
            tools_deny: fs.tools_deny.clone(),
            ..ServerConfig::default()
        }),
        _ => Err(ProfileError::InvalidServer {
            name: SERVER_MEMORY.to_string(),
            message: format!(
                "servers.memory: invalid mode \"{mode}\" (must be one of {MEMORY_MODE_READ_ONLY}, {MEMORY_MODE_READ_WRITE})"
            ),
        }),
    }
}

fn resolve_skills(fs: &RawServer) -> (ServerConfig, Vec<String>) {
    let mut warnings = Vec::new();
    if !fs.mode.is_empty() {
        warnings.push(format!(
            "servers.skills: mode \"{}\" is ignored (skills has no modes)",
            fs.mode
        ));
    }
    (
        ServerConfig {
            enabled: fs.enabled.unwrap_or(false),
            mode: String::new(),
            tools_allow: fs.tools_allow.clone(),
            tools_deny: fs.tools_deny.clone(),
            ..ServerConfig::default()
        },
        warnings,
    )
}

fn resolve_usage(fs: &RawServer) -> (ServerConfig, Vec<String>) {
    let mut warnings = Vec::new();
    if !fs.mode.is_empty() {
        warnings.push(format!(
            "servers.usage: mode \"{}\" is ignored (usage has no modes)",
            fs.mode
        ));
    }
    (
        ServerConfig {
            enabled: fs.enabled.unwrap_or(false),
            mode: String::new(),
            tools_allow: fs.tools_allow.clone(),
            tools_deny: fs.tools_deny.clone(),
            ..ServerConfig::default()
        },
        warnings,
    )
}

fn resolve_optional(fs: &RawServer) -> ServerConfig {
    ServerConfig {
        enabled: fs.enabled.unwrap_or(false),
        tools_allow: fs.tools_allow.clone(),
        tools_deny: fs.tools_deny.clone(),
        ..ServerConfig::default()
    }
}

fn resolve_foreign(
    alias: &str,
    fs: &RawServer,
) -> Result<(ServerConfig, Vec<String>), ProfileError> {
    if fs.command.is_empty() && fs.url.is_empty() {
        return Err(ProfileError::InvalidServer {
            name: alias.to_string(),
            message: format!(
                "servers.{alias}: foreign server requires command (with optional args) or url"
            ),
        });
    }

    let mut warnings = Vec::new();
    if !fs.mode.is_empty() {
        warnings.push(format!(
            "servers.{alias}: mode \"{}\" is ignored (foreign servers have no mode presets)",
            fs.mode
        ));
    }

    let access = if fs.access.is_empty() {
        FOREIGN_ACCESS_WRITE.to_string()
    } else {
        fs.access.clone()
    };

    match access.as_str() {
        FOREIGN_ACCESS_READ | FOREIGN_ACCESS_WRITE => Ok((
            ServerConfig {
                enabled: fs.enabled.unwrap_or(false),
                command: fs.command.clone(),
                args: fs.args.clone(),
                url: fs.url.clone(),
                access,
                tools_read: fs.tools_read.clone(),
                tools_write: fs.tools_write.clone(),
                tools_allow: fs.tools_allow.clone(),
                tools_deny: fs.tools_deny.clone(),
                mode: String::new(),
            },
            warnings,
        )),
        _ => Err(ProfileError::InvalidServer {
            name: alias.to_string(),
            message: format!(
                "servers.{alias}: invalid access \"{access}\" (must be \"{FOREIGN_ACCESS_READ}\" or \"{FOREIGN_ACCESS_WRITE}\")"
            ),
        }),
    }
}
