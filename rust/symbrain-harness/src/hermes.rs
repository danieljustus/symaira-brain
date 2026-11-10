//! Read-only discovery of Hermes configuration files.
//!
//! Hermes keeps its MCP server registrations in `config.yaml`. The root is
//! `$HERMES_HOME` when set, else `~/.hermes`; every profile adds
//! `<root>/profiles/<name>/config.yaml`. Symbrain only reads these files:
//! `symbrain install`/`uninstall` do not write Hermes configs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Environment variable that relocates the Hermes root directory.
pub const HERMES_HOME_ENV: &str = "HERMES_HOME";

/// One profile-scoped Hermes configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesProfile {
    /// Profile directory name under `<root>/profiles`.
    pub name: String,
    /// Path of the profile's `config.yaml`.
    pub config: PathBuf,
}

/// Where Hermes keeps its configuration files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesLocations {
    /// `$HERMES_HOME` or `~/.hermes`.
    pub root: PathBuf,
    /// The default (global) `config.yaml`.
    pub global: PathBuf,
    /// Profiles that have a `config.yaml`, sorted by name.
    pub profiles: Vec<HermesProfile>,
}

/// Resolves Hermes locations from the process environment.
///
/// Returns `None` when neither `HERMES_HOME` nor a usable `HOME` is set.
#[must_use]
pub fn hermes_locations() -> Option<HermesLocations> {
    locations_from(std::env::var_os(HERMES_HOME_ENV), std::env::var_os("HOME"))
}

/// Resolves Hermes locations from explicit `HERMES_HOME` and `HOME` values.
#[must_use]
pub fn locations_from(
    hermes_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<HermesLocations> {
    let root = hermes_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|home| Path::new(&home).join(".hermes"))
        })?;
    Some(HermesLocations {
        global: root.join("config.yaml"),
        profiles: profiles_under(&root),
        root,
    })
}

fn profiles_under(root: &Path) -> Vec<HermesProfile> {
    let Ok(entries) = std::fs::read_dir(root.join("profiles")) else {
        return Vec::new();
    };
    let mut profiles: Vec<HermesProfile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let config = entry.path().join("config.yaml");
            // Hidden entries are not profiles; a profile without a config
            // registers no MCP servers, so there is nothing to report.
            (!name.starts_with('.') && std::fs::metadata(&config).is_ok_and(|m| m.is_file()))
                .then_some(HermesProfile { name, config })
        })
        .collect();
    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    profiles
}
