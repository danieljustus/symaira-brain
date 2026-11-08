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

use super::base::base_path_for_scope;
use super::destination::root_metadata;
use super::lock;
use super::replace::open_trusted_dir;
use super::status_compare::{
    compare_one, is_regular_skill_source, read_entries, resolve_link, resolves_to_directory,
    unmanaged,
};
use crate::BundleLoader;
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
    /// Comparison of the current library render with the retained render cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_status: Option<super::render_status::RenderStatus>,
    /// Changed paths and hashes; skill content is never returned here.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub render_drift: Vec<super::render_status::RenderDrift>,
    /// Why an existing render cache could not be compared safely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_error: Option<String>,
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
    /// Optional trusted render cache root for an additional read-only comparison.
    pub render_dir: Option<PathBuf>,
    /// Compare against Go's cached status render.
    ///
    /// Go's `skills status`/`skills sync` pass a cache directory, which makes the
    /// coarse source-hash comparison (used when no base snapshot exists) read the
    /// cached render's fingerprint-based hash instead of the install-time hash.
    /// Callers that mirror those verbs set this; the MCP restore path, which
    /// passes no cache directory in Go, leaves it unset.
    pub cached_render_hash: bool,
    /// Optional exact names to scan.
    pub skills: Vec<String>,
}

/// Scans every requested target and scope in deterministic order.
pub fn status(options: &StatusOptions) -> Result<Vec<InstallStatus>, SkillError> {
    status_with_loader(options, &BundleLoader::default())
}

pub(super) fn status_with_loader(
    options: &StatusOptions,
    loader: &BundleLoader,
) -> Result<Vec<InstallStatus>, SkillError> {
    let targets = if options.targets.is_empty() {
        crate::target::target_names()
    } else {
        options.targets.clone()
    };
    let mut rows = Vec::new();
    for target in targets {
        let mut one = options.clone();
        one.targets = vec![target.clone()];
        rows.extend(status_target(&one, &target, loader)?);
    }
    rows.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(rows)
}

/// Go's `os.ReadDir` failure for a skill root the confined opener refused:
/// `scan skill root <root>: open <root>: <errno text>`.
fn scan_root_error(root: &std::path::Path, error: SkillError) -> SkillError {
    match std::fs::read_dir(root) {
        Err(source) => SkillError(format!(
            "scan skill root {}: open {}: {}",
            root.display(),
            root.display(),
            crate::gotext::io_text(&source)
        )),
        Ok(_) => error,
    }
}

fn status_target(
    options: &StatusOptions,
    target: &str,
    loader: &BundleLoader,
) -> Result<Vec<InstallStatus>, SkillError> {
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
    // An absent install root is a no-op and status must not create it merely
    // to acquire a read lock. If it exists, lock both shared directory roots
    // before reading entries or their per-skill base manifests.
    if root_metadata(&root)
        .map_err(|error| scan_root_error(&root, error))?
        .is_none()
    {
        return Ok(Vec::new());
    }
    let base_root = base_path_for_scope(
        &options.home_dir,
        options.base_dir.as_deref(),
        target,
        scope,
        "status-placeholder",
        options.project_dir.as_deref(),
    )?
    .parent()
    .ok_or_else(|| SkillError(format!("{target} base root has no parent")))?
    .to_path_buf();
    let mut lock_roots = vec![root.clone(), base_root];
    if let Some(render_root) = &options.render_dir {
        lock_roots.push(render_root.join(target));
    }
    if scope == "project" && options.project_dir.is_some() {
        let legacy_root = super::base::legacy_project_base_path(
            &options.home_dir,
            options.base_dir.as_deref(),
            target,
            "status-placeholder",
        )?
        .parent()
        .ok_or_else(|| SkillError(format!("{target} legacy base root has no parent")))?
        .to_path_buf();
        lock_roots.push(legacy_root);
    }
    let _locks = lock::acquire_existing_shared(&lock_roots)?;
    let Some(metadata) = root_metadata(&root).map_err(|error| scan_root_error(&root, error))?
    else {
        return Ok(Vec::new());
    };
    // Go's `os.ReadDir` follows a symlinked skill root; entry paths keep the
    // unresolved root prefix. A dangling link reads as a missing root.
    let open_root = if metadata.file_type().is_symlink() {
        match std::fs::canonicalize(&root) {
            Ok(resolved) => resolved,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(source) => {
                return Err(SkillError(format!(
                    "scan skill root {}: open {}: {}",
                    root.display(),
                    root.display(),
                    crate::gotext::io_text(&source)
                )));
            }
        }
    } else {
        root.clone()
    };
    let root_cap = open_trusted_dir(&open_root).map_err(|error| scan_root_error(&root, error))?;
    let mut entries = read_entries(&root_cap).map_err(|error| scan_root_error(&root, error))?;
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
            match resolve_link(&open_root.join(&name_os)) {
                Some(resolved) if resolves_to_directory(&resolved)? => resolved,
                // Go reads the marker through the link; a link whose target is
                // missing or not a directory simply carries no marker and stays
                // unmanaged. Opening it as a directory would abort the scan.
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
        let observation = if file_type.is_symlink() {
            super::marker_go::read_at_path(&installed_tree)?
        } else {
            let skill_cap = root_cap
                .open_dir_nofollow(&name_os)
                .map_err(|error| SkillError(format!("open installed skill {name}: {error}")))?;
            super::marker_go::read(&skill_cap)
        };
        let Some(observation) = observation else {
            rows.push(unmanaged(target, &name, path));
            continue;
        };
        let marker = observation.marker;
        if let Some(error) = observation.error {
            // Go keeps the fields it decoded around a mistyped value and
            // reports the managed install as stale with the decode error.
            rows.push(InstallStatus {
                target: target.to_owned(),
                name: name.clone(),
                path: path.clone(),
                status: StatusKind::Stale,
                mode: Some(marker.mode.clone()).filter(|value| !value.is_empty()),
                installed_at: Some(marker.installed.clone()).filter(|value| !value.is_empty()),
                source_hash: Some(marker.source_hash.clone()).filter(|value| !value.is_empty()),
                allow_executable: marker.allow_executable.then_some(true),
                error: Some(format!("reading marker {}: {error}", path.display())),
                drift: Vec::new(),
                render_status: None,
                render_drift: Vec::new(),
                render_error: None,
            });
            continue;
        }
        // A source_hash-only marker is a legacy cache marker, not an owned
        // installation. Do not silently upgrade it into a managed entry.
        let marker_for_row = marker.clone();
        let common = |status, drift, error| InstallStatus {
            target: target.to_owned(),
            name: name.clone(),
            path: path.clone(),
            status,
            mode: Some(marker_for_row.mode.clone()).filter(|mode| !mode.is_empty()),
            installed_at: Some(marker_for_row.installed.clone()).filter(|value| !value.is_empty()),
            source_hash: Some(marker_for_row.source_hash.clone()).filter(|value| !value.is_empty()),
            allow_executable: marker_for_row.allow_executable.then_some(true),
            error,
            drift,
            render_status: None,
            render_drift: Vec::new(),
            render_error: None,
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
                installed_at: Some(marker.installed.clone()).filter(|value| !value.is_empty()),
                source_hash: Some(marker.source_hash.clone()).filter(|value| !value.is_empty()),
                allow_executable: marker.allow_executable.then_some(true),
                error: None,
                drift: Vec::new(),
                render_status: None,
                render_drift: Vec::new(),
                render_error: None,
            });
            continue;
        }
        let source = options.library_dir.join(&name);
        if !is_regular_skill_source(&source)? {
            rows.push(common(StatusKind::Orphaned, Vec::new(), None));
            continue;
        }
        match compare_one(
            &source,
            &installed_tree,
            &name,
            target,
            marker,
            options,
            loader,
            common,
        ) {
            Ok(row) => rows.push(row),
            // Go status keeps a broken managed install visible as a stale
            // row with its comparison diagnostic instead of aborting the
            // entire scan.
            Err(error) => rows.push(common(StatusKind::Stale, Vec::new(), Some(error.0))),
        }
    }
    Ok(rows)
}
