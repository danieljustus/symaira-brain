//! Capability-rooted atomic replacement of an `OpenCode` skill directory.
#![allow(
    clippy::doc_markdown,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_pass_by_value
)]

use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ambient_authority::ambient_authority;
#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::PermissionsExt;
use cap_std::fs::{Dir, OpenOptions};

use super::sync::{sync_dir, sync_tree};
use crate::model::{MAX_INPUT_SIZE, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A deterministic fault point used by atomicity tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// Fail while writing staged content.
    Write,
    /// Fail while syncing staged content.
    Sync,
    /// Fail while moving the old destination aside.
    Backup,
    /// Fail while promoting the staged destination.
    Install,
    /// Fail while removing the rollback backup.
    RemoveBackup,
}

/// Replaces `destination` with a copy of `source`, preserving the old tree on failure.
/// `source` and `destination` must be directory paths and destination parents are
/// opened component-by-component without following symlink ancestors.
pub(crate) fn replace_tree(
    source: &Path,
    destination: &Path,
    mode: u32,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let name = safe_name(destination.file_name())?;
    let parent = destination
        .parent()
        .ok_or_else(|| SkillError("destination has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    let source_root = open_trusted_dir(source)?;
    let existing = root.symlink_metadata(&name).ok();
    if existing
        .as_ref()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(SkillError("destination skill path is a symlink".to_owned()));
    }
    if existing.as_ref().is_some_and(|metadata| !metadata.is_dir()) {
        return Err(SkillError(
            "destination skill path is not a directory".to_owned(),
        ));
    }
    let stage = unique_name(".symskills-stage-")?;
    root.create_dir(&stage)
        .map_err(|error| SkillError(format!("create staging directory: {error}")))?;
    let result = (|| {
        let mut entries_seen = 0_usize;
        let mut total_bytes = 0_u64;
        copy_tree(
            &source_root,
            Path::new("."),
            &root,
            &stage,
            mode,
            fault,
            &mut entries_seen,
            &mut total_bytes,
        )?;
        sync_tree(&root, &stage, fault)?;
        let backup = if existing.is_some() {
            Some(unique_name(".symskills-backup-")?)
        } else {
            None
        };
        if let Some(backup) = &backup {
            fail(fault, FaultPoint::Backup)?;
            root.rename(&name, &root, backup)
                .map_err(|error| SkillError(format!("move previous install aside: {error}")))?;
        }
        if let Err(error) = fail(fault, FaultPoint::Install).and_then(|()| {
            root.rename(&stage, &root, &name)
                .map_err(|error| SkillError(format!("publish install: {error}")))
        }) {
            if let Some(backup) = &backup {
                let _ = root.rename(backup, &root, &name);
            }
            return Err(error);
        }
        if let Err(error) = sync_dir(&root, Path::new("."), fault) {
            let _ = root.remove_dir_all(&name);
            if let Some(backup) = &backup {
                let _ = root.rename(backup, &root, &name);
            }
            return Err(SkillError(format!("sync destination parent: {error}")));
        }
        if let Some(backup) = backup {
            if let Err(error) = fail(fault, FaultPoint::RemoveBackup) {
                let _ = root.remove_dir_all(&name);
                let _ = root.rename(&backup, &root, &name);
                return Err(error);
            }
            root.remove_dir_all(&backup)
                .map_err(|error| SkillError(format!("remove rollback backup: {error}")))?;
        }
        Ok(())
    })();
    if root.symlink_metadata(&stage).is_ok() {
        let _ = root.remove_dir_all(&stage);
    }
    result
}

fn fail(fault: Option<FaultPoint>, point: FaultPoint) -> Result<(), SkillError> {
    if fault == Some(point) {
        return Err(SkillError(format!("injected replacement fault: {point:?}")));
    }
    Ok(())
}

pub(crate) fn safe_name(name: Option<&std::ffi::OsStr>) -> Result<PathBuf, SkillError> {
    let name = name.ok_or_else(|| SkillError("destination has no name".to_owned()))?;
    if name.is_empty() || name == "." || name == ".." || name.to_string_lossy().contains('/') {
        return Err(SkillError("unsafe destination component".to_owned()));
    }
    Ok(PathBuf::from(name))
}

pub(crate) fn unique_name(prefix: &str) -> Result<PathBuf, SkillError> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!("{prefix}{}-{id:016x}", std::process::id());
    if name.len() > 255 {
        return Err(SkillError(
            "temporary name exceeds component limit".to_owned(),
        ));
    }
    Ok(PathBuf::from(name))
}

/// Opens an existing directory without creating components or following
/// symlink ancestors. Read-only callers use this to avoid changing status
/// scans merely by probing absent target roots.
pub(crate) fn open_existing_dir(path: &Path) -> Result<Dir, SkillError> {
    let absolute = std::path::absolute(path)
        .map_err(|error| SkillError(format!("resolve existing root: {error}")))?;
    let mut current = Dir::open_ambient_dir(Path::new("/"), ambient_authority())
        .map_err(|error| SkillError(format!("open filesystem root: {error}")))?;
    let mut first_normal = true;
    for component in absolute.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(SkillError("unsafe existing root path".to_owned()));
        };
        let next = if first_normal && name == "var" {
            current.open_dir(name)
        } else {
            current.open_dir_nofollow(name)
        };
        current = next.map_err(|error| SkillError(format!("open existing root: {error}")))?;
        first_normal = false;
    }
    Ok(current)
}

pub(crate) fn open_trusted_dir(path: &Path) -> Result<Dir, SkillError> {
    let absolute = std::path::absolute(path)
        .map_err(|error| SkillError(format!("resolve trusted root: {error}")))?;
    let mut current = Dir::open_ambient_dir(Path::new("/"), ambient_authority())
        .map_err(|error| SkillError(format!("open filesystem root: {error}")))?;
    let mut first_normal = true;
    for component in absolute.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(SkillError("unsafe trusted root path".to_owned()));
        };
        let next = if first_normal && name == "var" {
            current.open_dir(name)
        } else {
            current.open_dir_nofollow(name)
        };
        first_normal = false;
        current = match next {
            Ok(next) => next,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                current.create_dir(name).map_err(|error| {
                    SkillError(format!("create trusted root component: {error}"))
                })?;
                current
                    .open_dir_nofollow(name)
                    .map_err(|error| SkillError(format!("open trusted root component: {error}")))?
            }
            Err(error) => return Err(SkillError(format!("open trusted root: {error}"))),
        };
    }
    Ok(current)
}

fn ensure_dirs(root: &Dir, relative: &Path) -> Result<(), SkillError> {
    let mut current = root
        .try_clone()
        .map_err(|error| SkillError(format!("clone destination capability: {error}")))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(SkillError("unsafe relative output path".to_owned()));
        };
        match current.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(SkillError(format!("create output directory: {error}"))),
        }
        current = current
            .open_dir_nofollow(name)
            .map_err(|error| SkillError(format!("open output directory: {error}")))?;
    }
    Ok(())
}

pub(crate) fn copy_tree_for_base(
    source: &Path,
    destination_root: &Dir,
    destination: &Path,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let source_root = open_trusted_dir(source)?;
    let mut entries_seen = 0_usize;
    let mut total_bytes = 0_u64;
    copy_tree(
        &source_root,
        Path::new("."),
        destination_root,
        destination,
        0o777,
        fault,
        &mut entries_seen,
        &mut total_bytes,
    )
}

#[allow(clippy::only_used_in_recursion)]
#[allow(clippy::too_many_arguments)]
fn copy_tree(
    source_root: &Dir,
    source: &Path,
    destination_root: &Dir,
    destination: &Path,
    mode: u32,
    fault: Option<FaultPoint>,
    entries_seen: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), SkillError> {
    let mut entries = Vec::new();
    for entry in source_root
        .read_dir(source)
        .map_err(|error| SkillError(format!("read rendered tree: {error}")))?
    {
        if entries.len() >= MAX_RESOURCE_ENTRIES {
            return Err(SkillError("rendered tree exceeds entry limit".to_owned()));
        }
        entries.push(entry.map_err(|error| SkillError(format!("read rendered entry: {error}")))?);
    }
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        *entries_seen = entries_seen.saturating_add(1);
        if *entries_seen > MAX_RESOURCE_ENTRIES {
            return Err(SkillError("rendered tree exceeds entry limit".to_owned()));
        }
        let name = entry.file_name();
        let source_path = source.join(&name);
        let target_path = destination.join(&name);
        let metadata = source_root
            .symlink_metadata(&source_path)
            .map_err(|error| SkillError(format!("stat rendered entry: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillError("rendered tree contains a symlink".to_owned()));
        }
        if metadata.is_dir() {
            ensure_dirs(destination_root, &target_path)?;
            copy_tree(
                source_root,
                &source_path,
                destination_root,
                &target_path,
                mode,
                fault,
                entries_seen,
                total_bytes,
            )?;
            continue;
        }
        if !metadata.is_file() {
            return Err(SkillError(
                "rendered tree contains a special file".to_owned(),
            ));
        }
        let mut input_options = OpenOptions::new();
        input_options.read(true).follow(FollowSymlinks::No);
        let input = source_root
            .open_with(&source_path, &input_options)
            .map_err(|error| SkillError(format!("open rendered file: {error}")))?;
        let mut bytes = Vec::new();
        input
            .take(MAX_INPUT_SIZE.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| SkillError(format!("read rendered file: {error}")))?;
        if bytes.len() as u64 > MAX_INPUT_SIZE {
            return Err(SkillError(
                "rendered file exceeds maximum input size".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if *total_bytes > MAX_TOTAL_RESOURCE_BYTES {
            return Err(SkillError(
                "rendered tree exceeds maximum total size".to_owned(),
            ));
        }
        let permissions = metadata.permissions();
        #[cfg(unix)]
        let permissions = {
            let mut permissions = permissions;
            let mut bits = permissions.mode() & 0o777;
            bits &= mode | 0o666;
            permissions.set_mode(bits);
            permissions
        };
        ensure_dirs(
            destination_root,
            target_path.parent().unwrap_or(Path::new(".")),
        )?;
        super::replace_io::write_bounded(
            destination_root,
            &target_path,
            &bytes,
            &permissions,
            mode,
            fault,
        )?;
    }
    Ok(())
}

pub(crate) fn write_bytes(
    root: &Dir,
    path: &Path,
    bytes: &[u8],
    mode: u32,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    fail(fault, FaultPoint::Write)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.mode(mode);
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = root
        .open_with(path, &options)
        .map_err(|error| SkillError(format!("create staged file: {error}")))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|error| SkillError(format!("write staged file: {error}")))
}
