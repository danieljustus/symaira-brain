//! Transactional removal of managed installs and base snapshots.
#![allow(clippy::collapsible_if, clippy::missing_errors_doc)]

use std::path::{Path, PathBuf};

use super::base::base_path_for_scope;
use super::core::install_path_for;
use super::destination::{
    effective_home, effective_mode, entry_metadata, is_unmanaged, scope_name, tombstone_path,
};
use super::event::{event_for, record_event};
use super::lock;
use super::replace;
use super::transaction::{move_entry, remove_entry, remove_tree, restore_backup};
use super::{BASE_SCHEMA_VERSION, FaultPoint, InstallOptions};
use crate::model::SkillError;

/// Removes a managed installation and writes a base tombstone.
pub fn uninstall(target: &str, name: &str, options: &InstallOptions) -> Result<bool, SkillError> {
    let destination = install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope_name(options),
        name,
    );
    let event_path = destination.clone().unwrap_or_default();
    let result =
        destination.and_then(|destination| uninstall_inner(target, name, &destination, options));
    let (outcome, error) = match &result {
        Ok(_) => ("ok", String::new()),
        Err(error) => ("error", error.0.clone()),
    };
    if !options.dry_run {
        record_event(
            options.events_path.as_deref(),
            event_for(
                "uninstall",
                target,
                name,
                scope_name(options),
                effective_mode(options),
                &event_path,
                outcome,
                &error,
            ),
        );
    }
    result
}

pub(crate) fn uninstall_inner(
    target: &str,
    name: &str,
    destination: &Path,
    options: &InstallOptions,
) -> Result<bool, SkillError> {
    let metadata = entry_metadata(destination)?;
    let Some(_metadata) = metadata else {
        return Ok(false);
    };
    if is_unmanaged(destination, target, name)? {
        return Err(SkillError("refusing to remove unmanaged skill".to_owned()));
    }
    // A dry run is deliberately side-effect free: do not remove the
    // destination, mutate the base, create a tombstone, or append an event.
    if options.dry_run {
        return Ok(true);
    }
    let home = effective_home(&options.home_dir)?;
    let base = base_path_for_scope(
        &home,
        options.base_dir.as_deref(),
        target,
        scope_name(options),
        name,
        options.project_dir.as_deref(),
    )?;
    let legacy_base = if scope_name(options) == "project" && options.project_dir.is_some() {
        Some(super::base::legacy_project_base_path(
            &home,
            options.base_dir.as_deref(),
            target,
            name,
        )?)
    } else {
        None
    };
    let tombstone = tombstone_path(target, name, options)?;
    let mut lock_paths = vec![destination.to_path_buf(), base.clone(), tombstone.clone()];
    if let Some(legacy) = &legacy_base {
        if legacy != &base {
            lock_paths.push(legacy.clone());
        }
    }
    let _locks = lock::acquire(&lock_paths)?;

    // Move all old state aside first. Nothing is destroyed until the
    // tombstone has been durably published. This includes the pre-identity
    // project snapshot so migration cleanup is atomic as well.
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut move_one = |path: &Path, prefix: &str| -> Result<(), SkillError> {
        if let Some(backup) = move_entry(path, prefix)? {
            moved.push((path.to_path_buf(), backup));
        }
        Ok(())
    };
    let rollback = |moved: &mut Vec<(PathBuf, PathBuf)>| {
        let _ = remove_entry(&tombstone);
        for (original, backup) in moved.iter().rev() {
            let _ = remove_entry(original);
            let _ = restore_backup(backup, original);
        }
    };
    if let Err(error) = move_one(destination, ".symskills-uninstall-dest-") {
        rollback(&mut moved);
        return Err(error);
    }
    if let Err(error) = move_one(&base, ".symskills-uninstall-base-") {
        rollback(&mut moved);
        return Err(error);
    }
    if let Some(legacy) = &legacy_base {
        if legacy != &base {
            if let Err(error) = move_one(legacy, ".symskills-uninstall-legacy-base-") {
                rollback(&mut moved);
                return Err(error);
            }
        }
    }
    if let Err(error) = move_one(&tombstone, ".symskills-uninstall-tombstone-") {
        rollback(&mut moved);
        return Err(error);
    }
    if let Err(error) = write_tombstone(&tombstone, target, name, options.fault) {
        rollback(&mut moved);
        return Err(error);
    }

    if options.fault == Some(FaultPoint::RemoveBackup) {
        rollback(&mut moved);
        return Err(SkillError(
            "injected replacement fault: RemoveBackup".to_owned(),
        ));
    }
    for (_, backup) in moved {
        remove_tree(&backup)?;
    }
    Ok(true)
}

pub(crate) fn write_tombstone(
    path: &Path,
    target: &str,
    name: &str,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("tombstone has no parent".to_owned()))?;
    let root = replace::open_trusted_dir(parent)?;
    let stage = replace::unique_name(".symskills-tombstone-")?;
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": BASE_SCHEMA_VERSION,
        "target": target,
        "name": name,
        "removed_at": chrono::Utc::now().to_rfc3339(),
    }))
    .map_err(|error| SkillError(format!("encode tombstone: {error}")))?;
    replace::write_bytes(&root, &stage, &[bytes, vec![b'\n']].concat(), 0o644, fault)?;
    let name = replace::safe_name(path.file_name())?;
    root.rename(&stage, &root, &name)
        .map_err(|error| SkillError(format!("publish tombstone: {error}")))
}

/// Alias retained for callers that use verb-first naming.
pub fn uninstall_skill(
    target: &str,
    name: &str,
    options: &InstallOptions,
) -> Result<bool, SkillError> {
    uninstall(target, name, options)
}
