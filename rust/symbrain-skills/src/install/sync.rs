//! Durable directory synchronization helpers for staged publication.
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::collapsible_if)]
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use serde::Serialize;
#[cfg(unix)]
use std::os::fd::AsFd;

use super::replace::FaultPoint;
use super::sync_lock::acquire_pull_lock;
#[cfg(windows)]
use crate::cap_root::sync_windows_dir;
use crate::model::{MAX_RESOURCE_ENTRIES, SkillError};
use crate::{BundleLoader, RenderMetadata, render_target};

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
    #[cfg(unix)]
    {
        // cap-std directories are O_PATH capabilities on Linux. Reopen the
        // same directory through the retained capability so fsync receives a
        // usable directory fd without weakening no-follow traversal.
        let directory = root
            .open_dir_nofollow(path)
            .map_err(|error| SkillError(format!("open directory for sync: {error}")))?;
        let fd = rustix::fs::openat(
            directory.as_fd(),
            ".",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| SkillError(format!("open directory for sync: {error}")))?;
        rustix::fs::fsync(&fd)
            .map_err(|error| SkillError(format!("sync staged directory: {error}")))
    }
    #[cfg(windows)]
    {
        sync_windows_dir(root, path)
            .map_err(|error| SkillError(format!("open directory for sync: {error}")))
    }
    #[cfg(not(any(unix, windows)))]
    {
        root.open_dir_nofollow(path)
            .map_err(|error| SkillError(format!("open directory for sync: {error}")))?
            .into_std_file()
            .sync_all()
            .map_err(|error| SkillError(format!("sync staged directory: {error}")))
    }
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
    /// Where rendered artifacts are written, mirroring Go's
    /// `SyncOptions.RenderDir` (the CLI passes `config.Defaults().RenderDir`).
    /// `None` keeps the per-user render cache fallback.
    pub render_dir: Option<PathBuf>,
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
    #[serde(skip)]
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
    // Go defaults an omitted scope to user, even when a project directory is
    // available. Keep project paths opt-in instead of inferring them.
    let scope = if options.scope.is_empty() {
        "user".to_owned()
    } else {
        options.scope.clone()
    };
    let loader = BundleLoader::default();
    let statuses = super::status::status_with_loader(
        &super::status::StatusOptions {
            home_dir: options.home_dir.clone(),
            project_dir: options.project_dir.clone(),
            scope: scope.clone(),
            targets: options.targets.clone(),
            library_dir: options.library_dir.clone(),
            base_dir: options.base_dir.clone(),
            skills: options.skills.clone(),
            // Render drift is an observational report. Sync's three-way policy
            // and stored installation modes must retain their existing meaning.
            render_dir: None,
        },
        &loader,
    )?;
    sync_selected(options, &statuses)
}

/// Synchronizes the already selected, read-only status rows without rescanning.
/// This preserves callers' selection and stop order (notably restore sync).
/// # Errors
/// Returns invalid scope or installation errors as result rows.
pub fn sync_selected(
    options: &SyncOptions,
    statuses: &[super::InstallStatus],
) -> Result<Vec<SyncResult>, SkillError> {
    let scope = if options.scope.is_empty() {
        "user"
    } else {
        options.scope.as_str()
    };
    let loader = BundleLoader::default();
    let mut results = Vec::new();
    for status in statuses.iter().cloned() {
        if status.status == super::StatusKind::HarnessChanged {
            results.push(skipped(&status, "harness changed; use symskills pull"));
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
        results.push(reinstall(&status, options, &scope, mode, &loader));
    }
    Ok(results)
}

fn reinstall(
    status: &super::InstallStatus,
    options: &SyncOptions,
    scope: &str,
    mode: String,
    loader: &BundleLoader,
) -> SyncResult {
    let source = options.library_dir.join(&status.name);
    let bundle = match loader.load(&source) {
        Ok(bundle) => bundle,
        Err(error) => return failed(status, error.0),
    };
    let rendered = match render_target(&bundle, &status.target, &RenderMetadata::default()) {
        Ok(rendered) => rendered,
        Err(error) => return failed(status, error.0),
    };
    let pull_lock = match acquire_pull_lock(&options.home_dir, &status.target, &rendered.name) {
        Ok(lock) => lock,
        Err(error) => return skipped(status, &error.0),
    };
    let install = super::install_rendered(
        &bundle,
        &rendered,
        &super::InstallOptions {
            home_dir: options.home_dir.clone(),
            project_dir: (scope == "project")
                .then(|| options.project_dir.clone())
                .flatten(),
            base_dir: options.base_dir.clone(),
            render_dir: options.render_dir.clone(),
            mode,
            force: options.force,
            events_path: options.events_path.clone(),
            allow_executable: status.allow_executable.unwrap_or(false)
                || bundle.manifest.skill.allow_executable,
            ..Default::default()
        },
    );
    let result = match install {
        Ok(result) => SyncResult {
            target: status.target.clone(),
            name: status.name.clone(),
            path: result.path,
            action: result.action,
            mode: Some(result.mode),
            allow_executable: status.allow_executable,
            error: String::new(),
        },
        Err(error) => failed(status, error.0),
    };
    drop(pull_lock);
    result
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

#[cfg(all(test, unix))]
#[path = "sync_tests.rs"]
mod tests;
