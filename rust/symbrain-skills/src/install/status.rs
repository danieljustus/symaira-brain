//! Read-only status classification for user-scope `OpenCode` skill installs.
#![allow(clippy::too_many_lines)]
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_pass_by_value,
    clippy::match_same_arms
)]

use std::path::PathBuf;

use cap_fs_ext::DirExt;
use serde::Serialize;

use super::destination::{entry_exists, entry_metadata};
use super::marker::{MarkerState, read_marker_at};
use super::replace::open_trusted_dir;
use super::status_compare::{
    compare_one, is_regular_skill_source, marker_row, read_entries, resolve_link, unmanaged,
};
use crate::model::SkillError;

/// Status vocabulary exposed by the skills status command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusKind {
    /// Installed tree matches the current library and base.
    InSync,
    /// Library output changed while the harness tree stayed at the base.
    Stale,
    /// Harness-only edits were detected.
    HarnessChanged,
    /// Both sides changed differently.
    Conflict,
    /// Both sides changed to identical content.
    Converged,
    /// A managed install has no library source.
    Orphaned,
    /// No valid symskills marker exists.
    Unmanaged,
}

/// One deterministic status row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallStatus {
    /// Target identity (`opencode`).
    pub target: String,
    /// Installed directory name.
    pub name: String,
    /// Absolute installation path.
    pub path: PathBuf,
    /// Classification.
    pub status: StatusKind,
    /// Copy mode for valid markers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Marker installation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    /// Marker source hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Whether the original install retained executable resource bits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_executable: Option<bool>,
    /// Diagnostic for malformed markers or failed comparisons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Per-file drift for harness changes/conflicts.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub drift: Vec<super::drift::FileDrift>,
}

/// Inputs for a read-only status scan.
#[derive(Debug, Clone, Default)]
pub struct StatusOptions {
    /// User home used to resolve harness skill roots.
    pub home_dir: PathBuf,
    /// Optional project root for project-scope scans.
    pub project_dir: Option<PathBuf>,
    /// `user` (default) or `project`.
    pub scope: String,
    /// Optional target filter; empty scans every registered target.
    pub targets: Vec<String>,
    /// Library root containing `<name>/SKILL.md` directories.
    pub library_dir: PathBuf,
    /// Optional custom base snapshot root.
    pub base_dir: Option<PathBuf>,
    /// Optional exact names to scan.
    pub skills: Vec<String>,
}

/// Scans every requested target and scope in deterministic order.
pub fn status(options: &StatusOptions) -> Result<Vec<InstallStatus>, SkillError> {
    let targets = if options.targets.is_empty() {
        crate::target::target_names()
    } else {
        options.targets.clone()
    };
    let mut rows = Vec::new();
    for target in targets {
        let mut one = options.clone();
        one.targets = vec![target.clone()];
        rows.extend(status_target(&one, &target)?);
    }
    Ok(rows)
}

fn status_target(options: &StatusOptions, target: &str) -> Result<Vec<InstallStatus>, SkillError> {
    let scope = if options.scope.is_empty() {
        "user"
    } else {
        options.scope.as_str()
    };
    if scope == "project" && options.project_dir.is_none() {
        return Err(SkillError(
            "project scope requires a project directory".to_owned(),
        ));
    }
    let root = super::install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope,
        "status-placeholder",
    )?
    .parent()
    .ok_or_else(|| SkillError(format!("{target} install root has no parent")))?
    .to_path_buf();
    let metadata = entry_metadata(&root)?;
    let Some(metadata) = metadata else {
        return Ok(Vec::new());
    };
    if metadata.file_type().is_symlink() {
        return Err(SkillError("OpenCode skill root is a symlink".to_owned()));
    }
    let root_cap = open_trusted_dir(&root)?;
    let mut entries = read_entries(&root_cap)?;
    entries.sort_by_key(|entry| entry.file_name());
    let filter = options
        .skills
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut rows = Vec::new();
    for entry in entries {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().into_owned();
        if !filter.is_empty() && !filter.contains(&name) {
            continue;
        }
        let path = root.join(&name_os);
        let file_type = entry
            .file_type()
            .map_err(|error| SkillError(format!("stat installed skill {name}: {error}")))?;
        let installed_tree = if file_type.is_symlink() {
            match resolve_link(&path) {
                Some(resolved) if entry_exists(&resolved)? => resolved,
                _ => {
                    rows.push(unmanaged(target, &name, path));
                    continue;
                }
            }
        } else {
            if !file_type.is_dir() {
                continue;
            }
            path.clone()
        };
        let marker = if file_type.is_symlink() {
            super::marker::read_marker(&installed_tree)?
        } else {
            let skill_cap = root_cap
                .open_dir_nofollow(&name_os)
                .map_err(|error| SkillError(format!("open installed skill {name}: {error}")))?;
            read_marker_at(&skill_cap)?
        };
        let MarkerState::Valid(marker) = marker else {
            rows.push(marker_row(target, &name, path, marker));
            continue;
        };
        // A source_hash-only marker is a legacy cache marker, not an owned
        // installation. Do not silently upgrade it into a managed entry.
        let marker_for_row = marker.clone();
        let common = |status, drift, error| InstallStatus {
            target: target.to_owned(),
            name: name.clone(),
            path: path.clone(),
            status,
            mode: Some(marker_for_row.mode.clone()),
            installed_at: Some(marker_for_row.installed.clone()),
            source_hash: Some(marker_for_row.source_hash.clone()),
            allow_executable: marker_for_row.allow_executable.then_some(true),
            error,
            drift,
        };
        if marker.managed_by != "symskills"
            || marker.target != target
            || marker.name != name
            || (marker.mode != "copy" && marker.mode != "symlink")
        {
            rows.push(InstallStatus {
                target: target.to_owned(),
                name: name.clone(),
                path: path.clone(),
                status: StatusKind::Unmanaged,
                mode: Some(marker.mode.clone()).filter(|mode| !mode.is_empty()),
                installed_at: None,
                source_hash: None,
                allow_executable: None,
                error: None,
                drift: Vec::new(),
            });
            continue;
        }
        let source = options.library_dir.join(&name);
        if !is_regular_skill_source(&source)? {
            rows.push(common(StatusKind::Orphaned, Vec::new(), None));
            continue;
        }
        rows.push(compare_one(
            &source,
            &installed_tree,
            &name,
            target,
            marker,
            options,
            common,
        )?);
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}
