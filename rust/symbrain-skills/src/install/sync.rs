//! Durable directory synchronization helpers for staged publication.
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::collapsible_if)]
use std::path::{Path, PathBuf};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use serde::Serialize;

use super::replace::FaultPoint;
use crate::model::{MAX_RESOURCE_ENTRIES, SkillError};
use crate::{RenderMetadata, load_bundle, render_target};

fn fail(fault: Option<FaultPoint>, point: FaultPoint) -> Result<(), SkillError> {
    if fault == Some(point) {
        return Err(SkillError(format!("injected replacement fault: {point:?}")));
    }
    Ok(())
}

pub(crate) fn sync_tree(
    root: &Dir,
    path: &Path,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let mut entries = Vec::new();
    for entry in root
        .read_dir(path)
        .map_err(|error| SkillError(format!("read staged directory: {error}")))?
    {
        if entries.len() >= MAX_RESOURCE_ENTRIES {
            return Err(SkillError("staged tree exceeds entry limit".to_owned()));
        }
        entries.push(
            entry.map_err(|error| SkillError(format!("read staged directory entry: {error}")))?,
        );
    }
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        let child = path.join(entry.file_name());
        if root
            .symlink_metadata(&child)
            .map_err(|error| SkillError(format!("stat staged entry: {error}")))?
            .is_dir()
        {
            sync_tree(root, &child, fault)?;
        }
    }
    sync_dir(root, path, fault)
}

pub(crate) fn sync_dir(
    root: &Dir,
    path: &Path,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    fail(fault, FaultPoint::Sync)?;
    root.open_dir_nofollow(path)
        .map_err(|error| SkillError(format!("open directory for sync: {error}")))?
        .into_std_file()
        .sync_all()
        .map_err(|error| SkillError(format!("sync staged directory: {error}")))
}

/// Conflict policy used by [`sync`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Refuse to overwrite either side; this is the safe default.
    #[default]
    Abort,
    /// Let the current library render replace the installed tree.
    PreferSource,
    /// Preserve the installed tree and do not write the library.
    PreferTarget,
    /// Leave the conflict for an explicit user resolution.
    Manual,
}

/// Inputs for a fleet-wide synchronization pass.
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Portable skill library root.
    pub library_dir: PathBuf,
    /// User home used for target and base paths.
    pub home_dir: PathBuf,
    /// Optional project root for project-scope synchronization.
    pub project_dir: Option<PathBuf>,
    /// `user` (default) or `project`.
    pub scope: String,
    /// Empty means every registered target.
    pub targets: Vec<String>,
    /// Empty means every installed skill.
    pub skills: Vec<String>,
    /// Optional custom base root.
    pub base_dir: Option<PathBuf>,
    /// Copy or managed symlink; empty preserves each marker's mode.
    pub mode: String,
    /// Adopt unmanaged destinations when reinstalling.
    pub force: bool,
    /// Do not write destinations, bases, or event records.
    pub dry_run: bool,
    /// Safe policy is [`ConflictPolicy::Abort`].
    pub conflict_policy: ConflictPolicy,
    /// Optional JSONL event log.
    pub events_path: Option<PathBuf>,
}

/// One row returned by [`sync`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncResult {
    /// Target identity.
    pub target: String,
    /// Skill name.
    pub name: String,
    /// Installed path.
    pub path: PathBuf,
    /// `planned`, `installed`, `skipped`, or `failed`.
    pub action: String,
    /// Install mode when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Whether the original install retained executable bits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_executable: Option<bool>,
    /// Diagnostic for skipped or failed rows.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// Synchronizes stale library renders while preserving harness edits.
///
/// Status is collected before any write. Harness changes are always skipped;
/// conflicts are only replaced under `PreferSource`, matching the Go safe
/// default (`Abort`) and making the conflict decision explicit.
pub fn sync(options: &SyncOptions) -> Result<Vec<SyncResult>, SkillError> {
    let scope = if options.scope.is_empty() && options.project_dir.is_some() {
        "project".to_owned()
    } else {
        options.scope.clone()
    };
    let statuses = super::status(&super::status::StatusOptions {
        home_dir: options.home_dir.clone(),
        project_dir: options.project_dir.clone(),
        scope,
        targets: options.targets.clone(),
        library_dir: options.library_dir.clone(),
        base_dir: options.base_dir.clone(),
        skills: options.skills.clone(),
    })?;
    let mut results = Vec::new();
    for status in statuses {
        if status.status == super::StatusKind::HarnessChanged {
            results.push(skipped(&status, "harness changed; use pull"));
            continue;
        }
        if status.status == super::StatusKind::Conflict
            && options.conflict_policy != ConflictPolicy::PreferSource
        {
            results.push(skipped(&status, "conflict; resolve manually"));
            continue;
        }
        if status.status != super::StatusKind::Stale && status.status != super::StatusKind::Conflict
        {
            continue;
        }
        if status.status != super::StatusKind::Conflict {
            if let Some(error) = status.error.as_deref() {
                results.push(skipped(&status, error));
                continue;
            }
        }
        let mode = if options.mode.is_empty() {
            status.mode.clone().unwrap_or_else(|| "copy".to_owned())
        } else {
            options.mode.clone()
        };
        if options.dry_run {
            results.push(SyncResult {
                target: status.target,
                name: status.name,
                path: status.path,
                action: "planned".to_owned(),
                mode: Some(mode),
                allow_executable: status.allow_executable,
                error: String::new(),
            });
            continue;
        }
        let source = options.library_dir.join(&status.name);
        let bundle = match load_bundle(&source) {
            Ok(bundle) => bundle,
            Err(error) => {
                results.push(failed(&status, error.0));
                continue;
            }
        };
        let rendered = match render_target(&bundle, &status.target, &RenderMetadata::default()) {
            Ok(rendered) => rendered,
            Err(error) => {
                results.push(failed(&status, error.0));
                continue;
            }
        };
        let install = super::install_rendered(
            &bundle,
            &rendered,
            &super::InstallOptions {
                home_dir: options.home_dir.clone(),
                project_dir: options.project_dir.clone(),
                base_dir: options.base_dir.clone(),
                mode: mode.clone(),
                force: options.force,
                events_path: options.events_path.clone(),
                allow_executable: status.allow_executable.unwrap_or(false),
                ..Default::default()
            },
        );
        match install {
            Ok(result) => results.push(SyncResult {
                target: status.target,
                name: status.name,
                path: result.path,
                action: result.action,
                mode: Some(result.mode),
                allow_executable: status.allow_executable,
                error: String::new(),
            }),
            Err(error) => results.push(failed(&status, error.0)),
        }
    }
    Ok(results)
}

fn skipped(status: &super::InstallStatus, error: &str) -> SyncResult {
    SyncResult {
        target: status.target.clone(),
        name: status.name.clone(),
        path: status.path.clone(),
        action: "skipped".to_owned(),
        mode: status.mode.clone(),
        allow_executable: status.allow_executable,
        error: error.to_owned(),
    }
}

fn failed(status: &super::InstallStatus, error: String) -> SyncResult {
    SyncResult {
        target: status.target.clone(),
        name: status.name.clone(),
        path: status.path.clone(),
        action: "failed".to_owned(),
        mode: status.mode.clone(),
        allow_executable: status.allow_executable,
        error,
    }
}
