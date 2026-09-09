//! Destination validation, scope paths, and managed-install helpers.

use std::path::{Path, PathBuf};

use super::InstallOptions;
use super::base::base_path_for_scope;
use super::marker::{Marker, MarkerState as MarkerReadState, ensure_writable};
use super::replace::{open_existing_dir, open_trusted_dir, safe_name};
use crate::model::{SkillError, validate_skill_name};

pub(crate) fn check_destination(
    destination: &Path,
    target: &str,
    name: &str,
    force: bool,
) -> Result<(), SkillError> {
    if entry_metadata(destination)?.is_none() {
        return Ok(());
    }
    if is_unmanaged(destination, target, name)? && !force {
        return Err(SkillError(
            "refusing to overwrite unmanaged skill".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn is_unmanaged(
    destination: &Path,
    target: &str,
    name: &str,
) -> Result<bool, SkillError> {
    let Some(metadata) = entry_metadata(destination)? else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() {
        let target_path = read_link_at(destination)?;
        if entry_metadata(&target_path)?.is_none() {
            return Ok(false);
        }
        return marker_ownership(&target_path, target, name);
    }
    marker_ownership(destination, target, name)
}

fn marker_ownership(path: &Path, target: &str, name: &str) -> Result<bool, SkillError> {
    match super::marker::read_marker(path)? {
        MarkerReadState::Valid(marker) => Ok(!marker_is_compatible(&marker, target, name)),
        MarkerReadState::Missing => Ok(true),
        state => {
            ensure_writable(&state, path)?;
            Ok(true)
        }
    }
}

pub(crate) fn marker_is_compatible(marker: &Marker, target: &str, name: &str) -> bool {
    if marker.managed_by.is_empty()
        && marker.target.is_empty()
        && marker.name.is_empty()
        && marker.mode.is_empty()
        && !marker.source_hash.is_empty()
    {
        return false;
    }
    marker.managed_by == "symskills"
        && (marker.target.is_empty() || marker.target == target)
        && (marker.name.is_empty() || marker.name == name)
}

pub(crate) fn source_for_snapshot(source: &Path) -> PathBuf {
    source.to_path_buf()
}

pub(crate) fn effective_mode(options: &InstallOptions) -> &str {
    if options.mode.is_empty() {
        "symlink"
    } else {
        options.mode.as_str()
    }
}

pub(crate) fn scope_name(options: &InstallOptions) -> &str {
    if options.project_dir.is_some() {
        "project"
    } else {
        "user"
    }
}

pub(crate) fn effective_home(home: &Path) -> Result<PathBuf, SkillError> {
    if home.as_os_str().is_empty() {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| SkillError("cannot determine user home directory".to_owned()))
    } else {
        Ok(home.to_path_buf())
    }
}

pub(crate) fn symlink_points_to(path: &Path, expected: &Path) -> Result<bool, SkillError> {
    if !is_symlink(path) {
        return Ok(false);
    }
    let actual = std::path::absolute(read_link_at(path)?)
        .map_err(|error| SkillError(format!("resolve destination symlink: {error}")))?;
    let expected = std::path::absolute(expected)
        .map_err(|error| SkillError(format!("resolve expected symlink: {error}")))?;
    Ok(actual == expected)
}

pub(crate) fn is_symlink(path: &Path) -> bool {
    entry_metadata(path).is_ok_and(|metadata| metadata.is_some_and(|m| m.file_type().is_symlink()))
}

pub(crate) fn read_link_at(path: &Path) -> Result<PathBuf, SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("symlink has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    let name = safe_name(path.file_name())?;
    root.read_link_contents(&name)
        .map_err(|error| SkillError(format!("read destination symlink: {error}")))
        .map(|link| {
            if link.is_absolute() {
                link
            } else {
                parent.join(link)
            }
        })
}

pub(crate) fn entry_exists(path: &Path) -> Result<bool, SkillError> {
    Ok(entry_metadata(path)?.is_some())
}

pub(crate) fn entry_metadata(path: &Path) -> Result<Option<cap_std::fs::Metadata>, SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("path has no parent".to_owned()))?;
    let root = match open_existing_dir(parent) {
        Ok(root) => root,
        Err(error) if error.0.contains("No such file or directory") => return Ok(None),
        Err(error) => return Err(error),
    };
    let name = safe_name(path.file_name())?;
    match root.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SkillError(format!("stat lifecycle path: {error}"))),
    }
}

pub(crate) fn adoption_backup(home: &Path, name: &str) -> Result<PathBuf, SkillError> {
    validate_skill_name(name)?;
    let root_path = home.join(".local/share/symskills/backups");
    let root = open_trusted_dir(&root_path)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    for index in 0..1024_u32 {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!("-{index}")
        };
        let candidate = format!("{name}-{stamp}{suffix}");
        if root.symlink_metadata(&candidate).is_err() {
            return Ok(root_path.join(candidate));
        }
    }
    Err(SkillError(
        "unable to allocate adoption backup path".to_owned(),
    ))
}

pub(crate) fn tombstone_path(
    target: &str,
    name: &str,
    options: &InstallOptions,
) -> Result<PathBuf, SkillError> {
    Ok(base_path_for_scope(
        &effective_home(&options.home_dir)?,
        options.base_dir.as_deref(),
        target,
        scope_name(options),
        name,
        options.project_dir.as_deref(),
    )?
    .with_extension("tombstone"))
}

pub(crate) fn remove_tombstone(
    target: &str,
    name: &str,
    options: &InstallOptions,
) -> Result<(), SkillError> {
    remove_file_at(&tombstone_path(target, name, options)?)?;
    if scope_name(options) == "project" && options.project_dir.is_some() {
        let legacy = super::base::legacy_project_base_path(
            &effective_home(&options.home_dir)?,
            options.base_dir.as_deref(),
            target,
            name,
        )?
        .with_extension("tombstone");
        remove_file_at(&legacy)?;
    }
    Ok(())
}

fn remove_file_at(path: &Path) -> Result<(), SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("path has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    let name = safe_name(path.file_name())?;
    match root.remove_file(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SkillError(format!("remove tombstone: {error}"))),
    }
}
