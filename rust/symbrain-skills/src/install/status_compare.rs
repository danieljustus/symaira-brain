//! Three-way comparison and status-row helpers.
#![allow(clippy::match_same_arms, clippy::needless_pass_by_value)]

use std::io;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;

use super::base::{manifest_hashes, read_manifest_for};
use super::destination::read_link_at;
use super::drift::{DriftKind, DriftSummary, classify_drift, file_hashes, summarize};
use super::marker::MarkerState;
use super::replace::open_trusted_dir;
use super::status::{InstallStatus, StatusKind, StatusOptions};
use crate::load_bundle;
use crate::materialize::materialize;
use crate::model::{MAX_RESOURCE_ENTRIES, SkillError};
use crate::render::{RenderMetadata, render_target};

pub(super) fn read_entries(root: &Dir) -> Result<Vec<cap_std::fs::DirEntry>, SkillError> {
    let mut entries = Vec::new();
    for entry in root.read_dir(".").map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            SkillError("OpenCode skill root not found".to_owned())
        } else {
            SkillError(format!("scan OpenCode skill root: {error}"))
        }
    })? {
        if entries.len() >= MAX_RESOURCE_ENTRIES {
            return Err(SkillError(
                "OpenCode skill root exceeds entry limit".to_owned(),
            ));
        }
        entries
            .push(entry.map_err(|error| SkillError(format!("scan OpenCode skill root: {error}")))?);
    }
    Ok(entries)
}

pub(super) fn is_regular_skill_source(path: &Path) -> Result<bool, SkillError> {
    let metadata = super::destination::entry_metadata(path)?;
    let Some(metadata) = metadata else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(false);
    }
    let root = open_trusted_dir(path)?;
    let metadata = match root.symlink_metadata("SKILL.md") {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(SkillError(format!("stat source SKILL.md: {error}"))),
    };
    Ok(metadata.is_file() && !metadata.file_type().is_symlink())
}

pub(super) fn compare_one<F>(
    source: &Path,
    installed: &Path,
    name: &str,
    target: &str,
    marker: super::marker::Marker,
    options: &StatusOptions,
    common: F,
) -> Result<InstallStatus, SkillError>
where
    F: Fn(StatusKind, Vec<super::drift::FileDrift>, Option<String>) -> InstallStatus,
{
    let bundle = load_bundle(source)?;
    let rendered = render_target(&bundle, target, &RenderMetadata::default())?;
    let temp = tempfile::tempdir()
        .map_err(|error| SkillError(format!("create status staging: {error}")))?;
    let fresh = materialize(&bundle, &rendered, temp.path())?;
    if let Some(manifest) = read_manifest_for(
        &options.home_dir,
        options.base_dir.as_deref(),
        target,
        if options.scope.is_empty() {
            "user"
        } else {
            options.scope.as_str()
        },
        name,
        options.project_dir.as_deref(),
    )? {
        let base_hashes = manifest_hashes(&manifest);
        let left = file_hashes(&fresh.root, false)?;
        let right = file_hashes(installed, false)?;
        let drifts = classify_drift(&base_hashes, &left, &right);
        let summary = summarize(&drifts);
        let (status, include_drift, error) = match summary {
            DriftSummary::InSync => (StatusKind::InSync, false, None),
            DriftSummary::Stale => (StatusKind::Stale, false, None),
            DriftSummary::HarnessChanged => (StatusKind::HarnessChanged, true, None),
            DriftSummary::Conflict => (StatusKind::Conflict, true, Some(conflict_error(&drifts))),
            DriftSummary::Converged => (StatusKind::InSync, false, None),
        };
        return Ok(common(
            status,
            if include_drift { drifts } else { Vec::new() },
            error,
        ));
    }
    let fresh_hash = fresh.source_hash;
    if !marker.source_hash.is_empty() {
        return Ok(common(
            if marker.source_hash == fresh_hash {
                StatusKind::InSync
            } else {
                StatusKind::Stale
            },
            Vec::new(),
            None,
        ));
    }
    let left = file_hashes(&fresh.root, false)?;
    let right = file_hashes(installed, false)?;
    let status = if left == right {
        StatusKind::InSync
    } else {
        StatusKind::Stale
    };
    Ok(common(status, Vec::new(), None))
}

pub(super) fn conflict_error(drifts: &[super::drift::FileDrift]) -> String {
    let names = drifts
        .iter()
        .filter(|drift| drift.kind == DriftKind::Conflict)
        .map(|drift| drift.path.as_str())
        .collect::<Vec<_>>();
    format!("conflict in: {}", names.join(", "))
}

pub(super) fn go_json_error(error: String) -> String {
    match error.as_str() {
        "expected value at line 1 column 1" => {
            "invalid character 'o' looking for beginning of value".to_owned()
        }
        "key must be a string at line 1 column 2" => {
            "invalid character 'b' looking for beginning of object key string".to_owned()
        }
        _ => error,
    }
}

pub(super) fn resolve_link(path: &Path) -> Option<PathBuf> {
    read_link_at(path).ok()
}

pub(super) fn unmanaged(target: &str, name: &str, path: PathBuf) -> InstallStatus {
    InstallStatus {
        target: target.to_owned(),
        name: name.to_owned(),
        path,
        status: StatusKind::Unmanaged,
        mode: None,
        installed_at: None,
        source_hash: None,
        allow_executable: None,
        error: None,
        drift: Vec::new(),
    }
}

pub(super) fn marker_row(
    target: &str,
    name: &str,
    path: PathBuf,
    state: MarkerState,
) -> InstallStatus {
    let error = match state {
        MarkerState::Missing => None,
        MarkerState::Malformed(error) => Some(format!(
            "reading marker {}: {}",
            path.display(),
            go_json_error(error),
        )),
        MarkerState::UnsupportedSchema(version) => {
            Some(format!("unsupported marker schema_version {version}"))
        }
        MarkerState::Valid(_) => None,
    };
    InstallStatus {
        target: target.to_owned(),
        name: name.to_owned(),
        path,
        status: if error.is_some() {
            StatusKind::Stale
        } else {
            StatusKind::Unmanaged
        },
        mode: None,
        installed_at: None,
        source_hash: None,
        allow_executable: None,
        error,
        drift: Vec::new(),
    }
}
