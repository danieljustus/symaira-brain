//! Native, read-only-by-default harness configuration primitives.
#![deny(unsafe_code)]
#![deny(missing_docs)]

mod backup;
mod diff;
mod document;
mod entry;
mod inventory;
mod inventory_read;
mod json;
mod json_parser;
mod registry;
mod server_info;
mod toml_backend;
mod write;

pub use backup::{backup, backup_at};
pub use diff::unified_diff;
pub use document::{Document, empty, load, parse};
pub use entry::{Entry, SUPERSEDED_CORE_NAMES};
pub use inventory::{
    Binding, BindingScan, BindingScanError, ConfigInventory, HarnessInventory, Inventory, list,
    list_for_env, profile_bindings,
};
pub use registry::{
    ConfigLocation, Format, Harness, HarnessName, InstructionAdapter, Name, SERVER_NAME,
    SkillTarget, all, lookup, names,
};
pub use server_info::ServerInfo;
pub use write::{
    AtomicFile, FileSnapshot, MAX_CONFIG_BYTES, atomic_write, backup_path, create_private_dir_all,
};

use std::fmt;
use std::io;

/// Errors returned by the harness domain without exposing config secrets.
#[derive(Debug)]
pub enum HarnessError {
    /// Filesystem access failed.
    Io(io::Error),
    /// JSON input was malformed or exceeded a safety limit.
    Json(String),
    /// TOML input was malformed; the message is redacted.
    Toml(String),
    /// A harness name is not present in the registry.
    UnknownHarness(String),
    /// The requested operation is unsupported for the harness.
    Unsupported(String),
    /// Serialization failed.
    Encode(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Json(e) => write!(f, "JSON parse error: {e}"),
            Self::Toml(e) => write!(f, "TOML parse error: {e}"),
            Self::UnknownHarness(name) => write!(
                f,
                "unknown harness {name:?}; want one of: {}",
                names().join(", ")
            ),
            Self::Unsupported(message) => f.write_str(message),
            Self::Encode(e) => write!(f, "encoding error: {e}"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<io::Error> for HarnessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml_edit::TomlError> for HarnessError {
    fn from(error: toml_edit::TomlError) -> Self {
        Self::Toml(error.message().to_owned())
    }
}

/// Resolves the trusted root for a registry path kind.
pub(crate) fn trusted_root(
    kind: registry::PathKind,
    target_os: &str,
    env: &[(String, String)],
) -> Result<std::path::PathBuf, HarnessError> {
    let get = |key: &str| {
        env.iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let home = || {
        get(if target_os == "windows" {
            "USERPROFILE"
        } else {
            "HOME"
        })
        .map(std::path::PathBuf::from)
        .ok_or_else(|| HarnessError::Unsupported("unable to resolve home directory".to_owned()))
    };
    match kind {
        registry::PathKind::Home(_) => home(),
        registry::PathKind::Xdg(_) => {
            if let Some(value) = get("XDG_CONFIG_HOME") {
                Ok(std::path::PathBuf::from(value))
            } else {
                let mut path = home()?;
                path.push(".config");
                Ok(path)
            }
        }
        registry::PathKind::ClaudeDesktop => match target_os {
            "darwin" | "macos" => home(),
            "windows" => {
                if let Some(value) = get("APPDATA") {
                    Ok(std::path::PathBuf::from(value))
                } else {
                    let mut path = home()?;
                    path.push("AppData");
                    path.push("Roaming");
                    Ok(path)
                }
            }
            _ => {
                if let Some(value) = get("XDG_CONFIG_HOME") {
                    Ok(std::path::PathBuf::from(value))
                } else {
                    let mut path = home()?;
                    path.push(".config");
                    Ok(path)
                }
            }
        },
        registry::PathKind::Unsupported => Err(HarnessError::Unsupported(
            "harness does not support MCP installation".to_owned(),
        )),
    }
}

/// Returns the native relative components for a registry path kind.
pub(crate) fn relative_path(kind: registry::PathKind, target_os: &str) -> std::path::PathBuf {
    let parts: &[&str] = match kind {
        registry::PathKind::Home(parts) | registry::PathKind::Xdg(parts) => parts,
        registry::PathKind::ClaudeDesktop => match target_os {
            "darwin" | "macos" => &[
                "Library",
                "Application Support",
                "Claude",
                "claude_desktop_config.json",
            ],
            _ => &["Claude", "claude_desktop_config.json"],
        },
        registry::PathKind::Unsupported => &[],
    };
    if target_os == "windows" {
        std::path::PathBuf::from(parts.join("\\"))
    } else {
        let mut path = std::path::PathBuf::new();
        for part in parts {
            path.push(part);
        }
        path
    }
}

/// Resolves a config path using an injected target OS and environment.
///
/// Unlike the Go test helper, the target OS controls both the home-variable
/// choice and path separator. This keeps Windows fallback tests correct when
/// run on Unix instead of copying the reference helper's host-GOOS bug.
pub(crate) fn resolve_path(
    kind: registry::PathKind,
    target_os: &str,
    env: &[(String, String)],
) -> Result<std::path::PathBuf, HarnessError> {
    let get = |key: &str| {
        env.iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let home = || {
        get(if target_os == "windows" {
            "USERPROFILE"
        } else {
            "HOME"
        })
        .map(str::to_owned)
        .ok_or_else(|| HarnessError::Unsupported("unable to resolve home directory".to_owned()))
    };
    let join = |base: String, parts: &[&str]| {
        if target_os == "windows" {
            std::path::PathBuf::from(
                std::iter::once(base)
                    .chain(parts.iter().map(|part| (*part).to_owned()))
                    .collect::<Vec<_>>()
                    .join("\\"),
            )
        } else {
            let mut path = std::path::PathBuf::from(base);
            for part in parts {
                path.push(part);
            }
            path
        }
    };
    let result = match kind {
        registry::PathKind::Home(parts) => join(home()?, parts),
        registry::PathKind::Xdg(parts) => {
            let base = match get("XDG_CONFIG_HOME") {
                Some(value) => value.to_owned(),
                None => join(home()?, &[".config"]).to_string_lossy().into_owned(),
            };
            join(base, parts)
        }
        registry::PathKind::ClaudeDesktop => match target_os {
            "darwin" | "macos" => join(
                home()?,
                &[
                    "Library",
                    "Application Support",
                    "Claude",
                    "claude_desktop_config.json",
                ],
            ),
            "windows" => {
                let base = match get("APPDATA") {
                    Some(value) => value.to_owned(),
                    None => join(home()?, &["AppData", "Roaming"])
                        .to_string_lossy()
                        .into_owned(),
                };
                join(base, &["Claude", "claude_desktop_config.json"])
            }
            _ => {
                let base = match get("XDG_CONFIG_HOME") {
                    Some(value) => value.to_owned(),
                    None => join(home()?, &[".config"]).to_string_lossy().into_owned(),
                };
                join(base, &["Claude", "claude_desktop_config.json"])
            }
        },
        registry::PathKind::Unsupported => {
            return Err(HarnessError::Unsupported(
                "harness does not support MCP installation".to_owned(),
            ));
        }
    };
    Ok(result)
}
