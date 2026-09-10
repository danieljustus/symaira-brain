//! Transactional destination publication for installs.
#![allow(
    clippy::assigning_clones,
    clippy::collapsible_if,
    clippy::too_many_lines
)]

use std::path::{Path, PathBuf};

use super::base::{base_path_for_scope, write_snapshot_for_scope};
use super::core::install_path_for;
use super::destination::{
    adoption_backup, check_destination, effective_home, effective_mode, entry_exists, is_symlink,
    is_unmanaged, read_link_at, remove_tombstone, scope_name, source_for_snapshot,
    symlink_points_to,
};
use super::lock;
use super::marker::{MarkerState as MarkerReadState, ensure_writable, marker_bytes};
use super::modes::{executable_changes, strip_executable_bits};
use super::replace;
use super::replace::replace_tree;
use super::transaction::{
    backup_existing, move_entry_to, remove_entry, remove_tree, restore_backup,
};
use super::{FaultPoint, InstallOptions, InstallResult, MARKER_FILE};
use crate::model::SkillError;

pub(crate) fn install_locks_for(
    target: &str,
    name: &str,
    options: &InstallOptions,
    extra: &[PathBuf],
) -> Result<lock::InstallLocks, SkillError> {
    let destination = install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope_name(options),
        name,
    )?;
    let base = base_path_for_scope(
        &effective_home(&options.home_dir)?,
        options.base_dir.as_deref(),
        target,
        scope_name(options),
        name,
        options.project_dir.as_deref(),
    )?;
    let mut paths = vec![destination, base];
    paths.extend_from_slice(extra);
    lock::acquire(&paths)
}

pub(crate) fn install_target(
    target: &str,
    source: &Path,
    name: &str,
    source_hash: &str,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    install_target_inner(target, source, name, source_hash, options)
}

pub(crate) fn preflight_destination(
    target: &str,
    name: &str,
    options: &InstallOptions,
) -> Result<(), SkillError> {
    let destination = install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope_name(options),
        name,
    )?;
    check_destination(&destination, target, name, options.force)
}

struct AdoptionGuard {
    backup: Option<PathBuf>,
    destination: PathBuf,
}

impl AdoptionGuard {
    fn empty(destination: PathBuf) -> Self {
        Self {
            backup: None,
            destination,
        }
    }

    fn new(backup: PathBuf, destination: PathBuf) -> Self {
        Self {
            backup: Some(backup),
            destination,
        }
    }

    fn commit(mut self) {
        self.backup = None;
    }
}

impl Drop for AdoptionGuard {
    fn drop(&mut self) {
        let Some(backup) = self.backup.take() else {
            return;
        };
        let _ = remove_entry(&self.destination);
        let _ = move_entry_to(&backup, &self.destination);
    }
}

pub(crate) fn install_target_inner(
    target: &str,
    source: &Path,
    name: &str,
    source_hash: &str,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    if options.dry_run {
        return install_target_inner_locked(target, source, name, source_hash, options);
    }
    let _locks = install_locks_for(target, name, options, &[])?;
    install_target_inner_locked(target, source, name, source_hash, options)
}

pub(crate) fn install_target_inner_locked(
    target: &str,
    source: &Path,
    name: &str,
    source_hash: &str,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let mode = effective_mode(options);
    if mode != "copy" && mode != "symlink" {
        return Err(SkillError(format!("unsupported install mode {mode:?}")));
    }
    let destination = install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope_name(options),
        name,
    )?;
    let changes = executable_changes(source, options.allow_executable)?;
    let mut result = InstallResult {
        action: if options.dry_run {
            "planned"
        } else {
            "installed"
        }
        .to_owned(),
        target: target.to_owned(),
        name: name.to_owned(),
        path: destination.clone(),
        mode: mode.to_owned(),
        mode_changes: changes,
        backup_path: None,
    };
    if options.dry_run {
        return Ok(result);
    }
    if !options.allow_executable {
        strip_executable_bits(source)?;
    }
    let base = base_path_for_scope(
        &effective_home(&options.home_dir)?,
        options.base_dir.as_deref(),
        target,
        scope_name(options),
        name,
        options.project_dir.as_deref(),
    )?;
    let destination_exists = entry_exists(&destination)?;
    if !(mode == "symlink" && symlink_points_to(&destination, source)?) {
        check_destination(&destination, target, name, options.force)?;
    }
    let mut adoption_guard = AdoptionGuard::empty(destination.clone());
    if destination_exists && options.force && is_unmanaged(&destination, target, name)? {
        let backup = adoption_backup(&effective_home(&options.home_dir)?, name)?;
        move_entry_to(&destination, &backup)
            .map_err(|error| SkillError(format!("back up unmanaged skill: {error}")))?;
        result.backup_path = Some(backup.clone());
        adoption_guard = AdoptionGuard::new(backup, destination.clone());
    }
    let previous = if entry_exists(&destination)? {
        let marker_path = if is_symlink(&destination) {
            let link = read_link_at(&destination)?;
            if link.is_absolute() {
                link
            } else {
                destination.parent().unwrap_or(Path::new(".")).join(link)
            }
        } else {
            destination.clone()
        };
        match super::marker::read_marker(&marker_path)? {
            MarkerReadState::Valid(marker) => Some(marker),
            MarkerReadState::Missing => None,
            state => {
                ensure_writable(&state, &marker_path)?;
                None
            }
        }
    } else {
        None
    };
    let marker_bytes = marker_bytes(
        target,
        name,
        source,
        source_hash,
        mode,
        options.allow_executable,
        previous,
    )?;
    // Hold the old destination and base until both publications succeed. This
    // makes destination and base one rollback transaction, not two independent
    // successful-looking updates.
    let destination_backup = if entry_exists(&destination)? {
        backup_existing(&destination, options.fault)?
    } else {
        None
    };
    let base_backup = if entry_exists(&base)? {
        backup_existing(&base, options.fault)?
    } else {
        None
    };
    let operation = (|| {
        if mode == "symlink" {
            install_symlink(source, &destination, &marker_bytes, options.fault)?;
        } else {
            install_copy_tree(source, &destination, &marker_bytes, options)?;
        }
        write_snapshot_for_scope(
            &source_for_snapshot(source),
            &base,
            target,
            name,
            options.project_dir.as_deref(),
            options.fault,
        )?;
        remove_tombstone(target, name, options)
    })();
    match operation {
        Ok(()) => {
            if let Some(backup) = destination_backup {
                remove_tree(&backup)?;
            }
            if let Some(backup) = base_backup {
                remove_tree(&backup)?;
            }
            adoption_guard.commit();
            Ok(result)
        }
        Err(error) => {
            if entry_exists(&destination)? {
                let _ = remove_tree(&destination);
            }
            if let Some(backup) = destination_backup {
                restore_backup(&backup, &destination).map_err(|restore| {
                    SkillError(format!("{error}; destination rollback failed: {restore}"))
                })?;
            }
            if entry_exists(&base)? {
                let _ = remove_tree(&base);
            }
            if let Some(backup) = base_backup {
                restore_backup(&backup, &base).map_err(|restore| {
                    SkillError(format!("{error}; base rollback failed: {restore}"))
                })?;
            }
            Err(error)
        }
    }
}

pub(crate) fn install_copy_tree(
    source: &Path,
    destination: &Path,
    marker: &[u8],
    options: &InstallOptions,
) -> Result<(), SkillError> {
    let destination_existed = entry_exists(destination)?;
    let backup = if destination_existed {
        backup_existing(destination, options.fault)?
    } else {
        None
    };
    let operation = (|| {
        replace_tree(
            source,
            destination,
            if options.allow_executable { 0o777 } else { 0 },
            options.fault,
        )?;
        let root = replace::open_trusted_dir(destination)?;
        replace::write_bytes(&root, Path::new(MARKER_FILE), marker, 0o644, options.fault)
    })();
    match operation {
        Ok(()) => {
            if options.fault == Some(FaultPoint::RemoveBackup) {
                if let Some(backup) = &backup {
                    let _ = remove_tree(destination);
                    restore_backup(backup, destination)?;
                    return Err(SkillError(
                        "injected replacement fault: RemoveBackup".to_owned(),
                    ));
                }
            }
            if let Some(backup) = backup {
                remove_tree(&backup)?;
            }
            Ok(())
        }
        Err(error) => {
            if entry_exists(destination)? {
                let _ = remove_tree(destination);
            }
            if let Some(backup) = backup {
                restore_backup(&backup, destination).map_err(|restore| {
                    SkillError(format!("{error}; destination rollback failed: {restore}"))
                })?;
            }
            Err(error)
        }
    }
}

pub(crate) fn install_symlink(
    source: &Path,
    destination: &Path,
    marker: &[u8],
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let parent = destination
        .parent()
        .ok_or_else(|| SkillError("destination has no parent".to_owned()))?;
    let root = replace::open_trusted_dir(parent)?;
    // Update the cache marker before publishing the link. The source is a
    // stable, capability-rooted render cache tree, never a user destination.
    replace::write_bytes(
        &replace::open_trusted_dir(source)?,
        Path::new(MARKER_FILE),
        marker,
        0o644,
        fault,
    )?;
    #[cfg(not(windows))]
    {
        let name = replace::safe_name(destination.file_name())?;
        let backup = backup_existing(destination, fault)?;
        let temp = replace::unique_name(".symskills-link-")?;
        if let Err(error) = root.symlink_contents(source, &temp) {
            if let Some(backup) = backup {
                let _ = restore_backup(&backup, destination);
            }
            return Err(SkillError(format!("create managed symlink: {error}")));
        }
        let result = root.rename(&temp, &root, &name).map_err(|error| {
            let _ = root.remove_file(&temp);
            SkillError(format!("publish managed symlink: {error}"))
        });
        if let Err(error) = result {
            if let Some(backup) = backup {
                let _ = restore_backup(&backup, destination);
            }
            return Err(error);
        }
        if fault == Some(FaultPoint::RemoveBackup) {
            if let Some(backup) = backup {
                let _ = remove_entry(destination);
                restore_backup(&backup, destination)?;
                return Err(SkillError(
                    "injected replacement fault: RemoveBackup".to_owned(),
                ));
            }
        }
        if let Some(backup) = backup {
            remove_tree(&backup)?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        let _ = fault;
        let _ = marker;
        Err(SkillError(
            "symlink install is unsupported on this platform".to_owned(),
        ))
    }
}
