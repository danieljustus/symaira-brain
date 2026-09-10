//! Pure three-way file drift classification and bounded tree hashing.
#![allow(clippy::redundant_closure_for_method_calls)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::destination::entry_metadata;
use super::marker::MARKER_FILE;
use super::replace::open_trusted_dir;
use crate::model::{MAX_INPUT_SIZE, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError};

/// A file's relationship to the frozen base, fresh library render, and target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriftKind {
    /// All present/absent states and bytes agree.
    Unchanged,
    /// Only the fresh library side changed.
    LibraryChanged,
    /// Only the installed harness side changed.
    HarnessChanged,
    /// Both sides changed to identical bytes.
    Converged,
    /// Both sides changed to different bytes.
    Conflict,
}

/// One path and its three digest values. An empty digest means absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDrift {
    /// Slash-separated relative path.
    pub path: String,
    /// Three-way classification.
    pub kind: DriftKind,
    /// Base digest, or empty when absent.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base: String,
    /// Fresh digest, or empty when absent.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub left: String,
    /// Installed digest, or empty when absent.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub right: String,
}

/// Classifies one path using the Go three-way table.
#[must_use]
pub fn classify_file(base: &str, left: &str, right: &str) -> DriftKind {
    match (base == left, base == right, left == right) {
        (true, true, _) => DriftKind::Unchanged,
        (false, true, _) => DriftKind::LibraryChanged,
        (true, false, _) => DriftKind::HarnessChanged,
        (false, false, true) => DriftKind::Converged,
        (false, false, false) => DriftKind::Conflict,
    }
}

/// Classifies the union of three digest maps in deterministic path order.
#[must_use]
pub fn classify_drift(
    base: &BTreeMap<String, String>,
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
) -> Vec<FileDrift> {
    let mut paths = BTreeSet::new();
    paths.extend(base.keys().cloned());
    paths.extend(left.keys().cloned());
    paths.extend(right.keys().cloned());
    paths
        .into_iter()
        .map(|path| {
            let base_hash = base.get(&path).cloned().unwrap_or_default();
            let left_hash = left.get(&path).cloned().unwrap_or_default();
            let right_hash = right.get(&path).cloned().unwrap_or_default();
            FileDrift {
                kind: classify_file(&base_hash, &left_hash, &right_hash),
                path,
                base: base_hash,
                left: left_hash,
                right: right_hash,
            }
        })
        .collect()
}

/// Counts the actionable result of a drift scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftSummary {
    /// No changes, including changes converged to identical bytes.
    InSync,
    /// Library-only change.
    Stale,
    /// Harness-only change.
    HarnessChanged,
    /// Both sides changed differently.
    Conflict,
    /// Both sides changed to the same bytes.
    Converged,
}

/// Summarizes a sorted drift list, with conflict taking precedence.
#[must_use]
pub fn summarize(drifts: &[FileDrift]) -> DriftSummary {
    let mut saw_library = false;
    let mut saw_harness = false;
    let mut saw_converged = false;
    for drift in drifts {
        match drift.kind {
            DriftKind::Conflict => return DriftSummary::Conflict,
            DriftKind::LibraryChanged => saw_library = true,
            DriftKind::HarnessChanged => saw_harness = true,
            DriftKind::Converged => saw_converged = true,
            DriftKind::Unchanged => {}
        }
    }
    match (saw_library, saw_harness, saw_converged) {
        (false, false, true | false) => DriftSummary::InSync,
        (false, true, _) => DriftSummary::HarnessChanged,
        (true, false, _) => DriftSummary::Stale,
        (true, true, _) => DriftSummary::Conflict,
    }
}

/// Hashes every regular file below a root without following symlink entries.
/// The marker and base manifest are bookkeeping, not skill content.
pub(crate) fn file_hashes(
    root: &Path,
    exclude_manifest: bool,
) -> Result<BTreeMap<String, String>, SkillError> {
    let mut output = BTreeMap::new();
    let Some(metadata) = entry_metadata(root)? else {
        return Ok(output);
    };
    if metadata.file_type().is_symlink() {
        return Err(SkillError(format!(
            "skill tree root is a symlink: {}",
            root.display()
        )));
    }
    let capability = open_trusted_dir(root)?;
    let mut entries = 0_usize;
    let mut total = 0_u64;
    walk_hashes(
        &capability,
        Path::new("."),
        exclude_manifest,
        &mut output,
        &mut entries,
        &mut total,
    )?;
    Ok(output)
}

fn walk_hashes(
    root: &Dir,
    current: &Path,
    exclude_manifest: bool,
    output: &mut BTreeMap<String, String>,
    entries: &mut usize,
    total: &mut u64,
) -> Result<(), SkillError> {
    let mut children = root
        .read_dir(current)
        .map_err(|error| SkillError(format!("read directory: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SkillError(format!("read directory entry: {error}")))?;
    if children.len() > MAX_RESOURCE_ENTRIES.saturating_sub(*entries) {
        return Err(SkillError(
            "skill tree exceeds maximum entry count".to_owned(),
        ));
    }
    children.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in children {
        *entries += 1;
        if *entries > MAX_RESOURCE_ENTRIES {
            return Err(SkillError(
                "skill tree exceeds maximum entry count".to_owned(),
            ));
        }
        let path = current.join(entry.file_name());
        let metadata = root
            .symlink_metadata(&path)
            .map_err(|error| SkillError(format!("stat tree entry: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillError(format!(
                "skill tree contains symlink {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            walk_hashes(root, &path, exclude_manifest, output, entries, total)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(SkillError(format!(
                "skill tree contains special file {}",
                path.display()
            )));
        }
        let relative = path
            .strip_prefix(".")
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative == MARKER_FILE || (exclude_manifest && relative == "manifest.json") {
            continue;
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = root
            .open_with(&path, &options)
            .map_err(|error| SkillError(format!("read {relative}: {error}")))?;
        let mut bytes = Vec::new();
        file.take(MAX_INPUT_SIZE.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| SkillError(format!("read {relative}: {error}")))?;
        if bytes.len() as u64 > MAX_INPUT_SIZE {
            return Err(SkillError(format!(
                "file {relative} exceeds maximum input size"
            )));
        }
        *total = total.saturating_add(bytes.len() as u64);
        if *total > MAX_TOTAL_RESOURCE_BYTES {
            return Err(SkillError(
                "skill tree exceeds maximum total size".to_owned(),
            ));
        }
        output.insert(relative, format!("{:x}", Sha256::digest(bytes)));
    }
    Ok(())
}
