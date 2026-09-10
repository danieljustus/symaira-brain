//! Public install inputs, results, and rendered-tree entry points.
#![allow(
    clippy::assigning_clones,
    clippy::cloned_ref_to_slice_refs,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]

use std::path::{Path, PathBuf};

use super::cache::cache_path;
use super::destination::{effective_home, effective_mode};
use super::event::record_install_outcome;
use super::ops::{
    install_locks_for, install_target, install_target_inner_locked, preflight_destination,
};
use super::replace::{FaultPoint, replace_tree};
use super::transaction::{backup_existing, remove_entry, remove_tree, restore_backup};
use crate::materialize::materialize;
use crate::model::{Bundle, SkillError, validate_skill_name};
use crate::render::Rendered;

/// Options for installing a rendered skill at any registered target and scope.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// User home used to resolve global harness skill roots.
    pub home_dir: PathBuf,
    /// Project root used for project-scope installs.
    pub project_dir: Option<PathBuf>,
    /// Optional replacement for the default base snapshot root.
    pub base_dir: Option<PathBuf>,
    /// `copy` (default) or managed `symlink`.
    pub mode: String,
    /// Adopt an unmanaged destination after moving it to a collision-free backup.
    pub force: bool,
    /// Report planned changes without writing destination or base bytes.
    pub dry_run: bool,
    /// Preserve executable bits on resource files.
    pub allow_executable: bool,
    /// Deterministic replacement fault for rollback tests.
    pub fault: Option<FaultPoint>,
    /// Optional JSONL operation-log path. Logging is best effort.
    pub events_path: Option<PathBuf>,
}

/// Result of an install attempt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstallResult {
    /// `installed` or `planned`.
    pub action: String,
    /// Harness target.
    pub target: String,
    /// Installed skill name.
    pub name: String,
    /// Absolute destination path.
    pub path: PathBuf,
    /// `copy` or `symlink`.
    pub mode: String,
    /// Files whose executable bits would be stripped.
    pub mode_changes: Vec<ModeChange>,
    /// Backup created when force adopts an unmanaged directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<PathBuf>,
}

/// One planned executable-bit change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModeChange {
    /// Slash-separated relative path.
    pub path: String,
    /// Original octal mode.
    pub from: String,
    /// Resulting octal mode.
    pub to: String,
}

/// Resolves the installation path for a registered target and scope.
pub fn install_path(home: &Path, name: &str) -> Result<PathBuf, SkillError> {
    install_path_for("opencode", home, None, "user", name)
}

/// Resolves a target's user- or project-scope installation path.
pub fn install_path_for(
    target: &str,
    home: &Path,
    project: Option<&Path>,
    scope: &str,
    name: &str,
) -> Result<PathBuf, SkillError> {
    validate_skill_name(name)?;
    let home = if home.as_os_str().is_empty() {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| SkillError("cannot determine user home directory".to_owned()))?
    } else {
        home.to_path_buf()
    };
    crate::target::skill_root(target, &home, project, scope)
        .map(|root| root.join(name))
        .ok_or_else(|| {
            SkillError(format!(
                "unknown target or missing project directory: {target:?}"
            ))
        })
}

/// Installs an OpenCode rendered tree in copy mode (the Phase 7.3A API).
pub fn install_copy(
    source: &Path,
    name: &str,
    source_hash: &str,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let mut copy_options = options.clone();
    if copy_options.mode.is_empty() {
        copy_options.mode = "copy".to_owned();
    }
    let result = install_target("opencode", source, name, source_hash, &copy_options);
    record_install_outcome("opencode", name, &copy_options, &result);
    result
}

/// Installs a rendered tree using its target identity.
pub fn install_rendered(
    bundle: &Bundle,
    rendered: &Rendered,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let result = install_rendered_inner(bundle, rendered, options);
    record_install_outcome(&rendered.target, &rendered.name, options, &result);
    result
}

fn install_rendered_inner(
    bundle: &Bundle,
    rendered: &Rendered,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let output = tempfile::tempdir()
        .map_err(|error| SkillError(format!("create install staging root: {error}")))?;
    let materialized = materialize(bundle, rendered, output.path())?;
    // A symlink must outlive the temporary materialization. Keep the rendered
    // tree under a stable per-user cache for that mode; copies can use the
    // private staging root and remain fully transactional.
    if effective_mode(options) == "symlink" && !options.dry_run {
        // Validate the destination before changing the cache. A rejected
        // unmanaged collision must not refresh or replace the cache tree.
        preflight_destination(&rendered.target, &rendered.name, options)?;
        let home = effective_home(&options.home_dir)?;
        let cache = home.join(".local/share/symskills/rendered");
        let stable = cache_path(&cache, rendered, options)?;
        let _locks =
            install_locks_for(&rendered.target, &rendered.name, options, &[stable.clone()])?;
        // Keep the previous cache tree until destination and base publication
        // both succeed. The install lock also serializes other processes.
        let cache_backup = backup_existing(&stable, None)?;
        let operation = (|| {
            replace_tree(
                &materialized.root,
                &stable,
                if options.allow_executable { 0o777 } else { 0 },
                options.fault,
            )?;
            install_target_inner_locked(
                &rendered.target,
                &stable,
                &rendered.name,
                &materialized.source_hash,
                options,
            )
        })();
        return match operation {
            Ok(result) => {
                if let Some(backup) = cache_backup {
                    remove_tree(&backup)?;
                }
                Ok(result)
            }
            Err(error) => {
                let _ = remove_entry(&stable);
                if let Some(backup) = cache_backup {
                    let _ = restore_backup(&backup, &stable);
                }
                Err(error)
            }
        };
    }
    install_target(
        &rendered.target,
        &materialized.root,
        &rendered.name,
        &materialized.source_hash,
        options,
    )
}
