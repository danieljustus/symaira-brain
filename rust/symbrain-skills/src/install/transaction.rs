//! Recoverable destination transaction helpers.

use std::path::{Path, PathBuf};

use super::replace::{FaultPoint, open_trusted_dir, safe_name, unique_name};
use crate::model::SkillError;

/// Moves an existing filesystem entry aside using a unique sibling name.
///
/// The move is performed through a retained parent directory capability, so a
/// destination symlink is moved as a link rather than followed. This is used
/// for both copied directories and managed symlink installs.
pub(crate) fn backup_existing(
    path: &Path,
    fault: Option<FaultPoint>,
) -> Result<Option<PathBuf>, SkillError> {
    let name = safe_name(path.file_name())?;
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("destination has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    if root.symlink_metadata(&name).is_err() {
        return Ok(None);
    }
    let backup = unique_name(".symskills-transaction-")?;
    if fault == Some(FaultPoint::Backup) {
        return Err(SkillError("injected replacement fault: Backup".to_owned()));
    }
    root.rename(&name, &root, &backup)
        .map_err(|error| SkillError(format!("move destination into transaction: {error}")))?;
    Ok(Some(parent.join(backup)))
}

/// Restores a transaction backup, removing the newly published destination.
pub(crate) fn restore_backup(backup: &Path, destination: &Path) -> Result<(), SkillError> {
    let name = safe_name(destination.file_name())?;
    let backup_name = safe_name(backup.file_name())?;
    let parent = destination
        .parent()
        .ok_or_else(|| SkillError("destination has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    remove_entry_at(&root, &name)?;
    root.rename(&backup_name, &root, &name)
        .map_err(|error| SkillError(format!("restore destination transaction: {error}")))
}

/// Removes a transaction backup without following path components.
pub(crate) fn remove_tree(path: &Path) -> Result<(), SkillError> {
    let name = safe_name(path.file_name())?;
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("transaction path has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    remove_entry_at(&root, &name)
}

/// Moves any existing entry aside, including a regular tombstone file.
pub(crate) fn move_entry(path: &Path, prefix: &str) -> Result<Option<PathBuf>, SkillError> {
    let name = safe_name(path.file_name())?;
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("transaction path has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    if root.symlink_metadata(&name).is_err() {
        return Ok(None);
    }
    let backup_name = unique_name(prefix)?;
    root.rename(&name, &root, &backup_name)
        .map_err(|error| SkillError(format!("move transaction entry: {error}")))?;
    Ok(Some(parent.join(backup_name)))
}

/// Moves an entry to an explicitly allocated destination through directory capabilities.
pub(crate) fn move_entry_to(source: &Path, destination: &Path) -> Result<(), SkillError> {
    let source_parent = source
        .parent()
        .ok_or_else(|| SkillError("source transaction path has no parent".to_owned()))?;
    let destination_parent = destination
        .parent()
        .ok_or_else(|| SkillError("destination transaction path has no parent".to_owned()))?;
    let source_root = open_trusted_dir(source_parent)?;
    let destination_root = open_trusted_dir(destination_parent)?;
    let source_name = safe_name(source.file_name())?;
    let destination_name = safe_name(destination.file_name())?;
    source_root
        .rename(&source_name, &destination_root, &destination_name)
        .map_err(|error| SkillError(format!("move transaction entry: {error}")))
}

/// Removes one path entry through its retained parent capability.
pub(crate) fn remove_entry(path: &Path) -> Result<(), SkillError> {
    let name = safe_name(path.file_name())?;
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("transaction path has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    remove_entry_at(&root, &name)
}

fn remove_entry_at(root: &cap_std::fs::Dir, name: &Path) -> Result<(), SkillError> {
    let Some(metadata) = root.symlink_metadata(name).ok() else {
        return Ok(());
    };
    let result = if metadata.file_type().is_symlink() || metadata.is_file() {
        root.remove_file(name)
    } else {
        root.remove_dir_all(name)
    };
    result.map_err(|error| SkillError(format!("remove transaction entry: {error}")))
}
