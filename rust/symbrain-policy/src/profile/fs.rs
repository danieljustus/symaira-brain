//! Filesystem operations for loading and listing profiles.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ProfileError;
use crate::profile::Profile;
use crate::profile::parse::{parse, parse_meta_name};
use crate::profile::validate::validate_name;

/// Outcome of attempting to load one profile file.
#[derive(Debug)]
pub struct LoadResult {
    /// Name of the profile.
    pub name: String,
    /// Successfully loaded profile, if any.
    pub profile: Option<Profile>,
    /// Error encountered when loading or validating, if any.
    pub err: Option<ProfileError>,
}

/// Returns the filesystem path for `name` under `$XDG_CONFIG_HOME/symbrain/profiles/<name>.toml`.
#[must_use]
pub fn path(name: &str) -> PathBuf {
    path_in(&symbrain_core::xdg::profiles_dir(), name)
}

/// Returns the filesystem path for `name` within a specific profiles directory.
#[must_use]
pub fn path_in(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.toml"))
}

/// Reports whether a profile file exists for `name` under the default profiles directory.
#[must_use]
pub fn exists(name: &str) -> bool {
    exists_in(&symbrain_core::xdg::profiles_dir(), name)
}

/// Reports whether a profile file exists for `name` in `dir`.
#[must_use]
pub fn exists_in(dir: &Path, name: &str) -> bool {
    path_in(dir, name).exists()
}

/// Reads, parses, and validates the profile named `name` from the XDG profiles directory.
///
/// # Errors
///
/// Returns [`ProfileError`] if name is invalid, file cannot be read, TOML is malformed,
/// or server invariants are violated.
pub fn load(name: &str) -> Result<Profile, ProfileError> {
    load_from_dir(&symbrain_core::xdg::profiles_dir(), name)
}

/// Reads, parses, and validates the profile named `name` within `dir`.
///
/// # Errors
///
/// Returns [`ProfileError`] if name is invalid, file cannot be read, TOML is malformed,
/// or server invariants are violated.
pub fn load_from_dir(dir: &Path, name: &str) -> Result<Profile, ProfileError> {
    validate_name(name)?;
    let p = path_in(dir, name);
    let data = fs::read_to_string(&p).map_err(|source| ProfileError::ReadFailed {
        path: p.clone(),
        source,
    })?;
    parse(name, &data)
}

/// Reads, parses, and validates the profile file at arbitrary `file_path`.
///
/// The profile name is derived from the file's own `[profile].name` field.
///
/// # Errors
///
/// Returns [`ProfileError`] if the file cannot be read, TOML is malformed,
/// the embedded profile name is missing or invalid, or server invariants are violated.
pub fn load_file(file_path: &Path) -> Result<Profile, ProfileError> {
    let data = fs::read_to_string(file_path).map_err(|source| ProfileError::ReadFailed {
        path: file_path.to_path_buf(),
        source,
    })?;

    let name = parse_meta_name(&data).map_err(|err| match err {
        ProfileError::ParseFailed { message, .. } => ProfileError::ParseFailed {
            name: None,
            path: Some(file_path.to_path_buf()),
            message,
        },
        other => other,
    })?;

    if let Err(err) = validate_name(&name) {
        let cause = match err {
            ProfileError::InvalidName { message, .. } => message,
            other => other.to_string(),
        };
        return Err(ProfileError::InvalidFileName {
            name: name.clone(),
            path: file_path.to_path_buf(),
            cause,
        });
    }

    parse(&name, &data)
}

/// Returns the sorted profile names found under the XDG profiles directory.
///
/// A missing profiles directory is not an error; it yields an empty vector.
///
/// # Errors
///
/// Returns [`ProfileError::ListFailed`] if reading the directory fails.
pub fn list_names() -> Result<Vec<String>, ProfileError> {
    list_names_in(&symbrain_core::xdg::profiles_dir())
}

/// Returns the sorted profile names found under `dir`.
///
/// A missing directory yields an empty vector.
///
/// # Errors
///
/// Returns [`ProfileError::ListFailed`] if reading the directory fails.
pub fn list_names_in(dir: &Path) -> Result<Vec<String>, ProfileError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(ProfileError::ListFailed {
                message: "failed to list profiles directory".to_string(),
                source: err,
            });
        }
    };

    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if let Some(stem) = name_str.strip_suffix(".toml") {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Loads and validates every profile found in the XDG profiles directory.
///
/// Reports per-file errors without failing the overall operation.
///
/// # Errors
///
/// Returns [`ProfileError`] only if listing the profiles directory itself fails.
pub fn load_all() -> Result<Vec<LoadResult>, ProfileError> {
    load_all_in(&symbrain_core::xdg::profiles_dir())
}

/// Loads and validates every profile found in `dir`.
///
/// # Errors
///
/// Returns [`ProfileError`] only if listing `dir` fails.
pub fn load_all_in(dir: &Path) -> Result<Vec<LoadResult>, ProfileError> {
    let names = list_names_in(dir)?;
    let mut results = Vec::with_capacity(names.len());
    for name in names {
        match load_from_dir(dir, &name) {
            Ok(p) => results.push(LoadResult {
                name,
                profile: Some(p),
                err: None,
            }),
            Err(err) => results.push(LoadResult {
                name,
                profile: None,
                err: Some(err),
            }),
        }
    }
    Ok(results)
}
