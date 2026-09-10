use std::path::{Component, Path};

use super::AdapterError;

/// Validates a project-relative target path without consulting the filesystem.
///
/// Both native path components and Windows separators are checked. This keeps
/// validation safe when a Windows-shaped path is tested on Unix and prevents
/// absolute paths, NUL bytes, and `..` traversal from becoming write targets.
///
/// # Errors
/// Returns [`AdapterError::InvalidTargetPath`] for an unsafe path.
pub fn validate_relative_target_path(path: &Path) -> Result<(), AdapterError> {
    let raw = path.to_string_lossy().into_owned();
    let invalid = || AdapterError::InvalidTargetPath(raw.clone());
    if raw.is_empty() || raw.contains('\0') || path.is_absolute() {
        return Err(invalid());
    }
    if raw.starts_with('/') || raw.starts_with('\\') || has_windows_drive_prefix(&raw) {
        return Err(invalid());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::RootDir | Component::Prefix(_) | Component::ParentDir
        )
    }) {
        return Err(invalid());
    }
    if raw.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(invalid());
    }
    if path.file_name().is_none() {
        return Err(invalid());
    }
    Ok(())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
