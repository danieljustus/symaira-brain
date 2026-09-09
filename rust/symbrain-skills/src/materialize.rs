//! Capability-rooted, atomic materialization of rendered skill bundles.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
#[cfg(unix)]
use cap_std::fs::{OpenOptionsExt, PermissionsExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::load::read_bundle_bytes;
use crate::model::{
    Bundle, MAX_INPUT_SIZE, MAX_RESOURCE_SIZE, MAX_TOTAL_RESOURCE_BYTES, SkillError,
};
use crate::render::Rendered;

const MAX_OUTPUT_ENTRIES: usize = crate::model::MAX_RESOURCE_ENTRIES + 16;
const MAX_OUTPUT_BYTES: u64 = MAX_TOTAL_RESOURCE_BYTES + MAX_INPUT_SIZE;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// One output file and its effective permission mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaterializedFile {
    /// Slash-separated path relative to the rendered skill root.
    pub path: String,
    /// Output mode in octal notation.
    pub mode: String,
    /// Exact output bytes.
    pub bytes: Vec<u8>,
}

/// Materialization result, including the marker hash and file manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Materialized {
    /// `<output>/<target>/<resolved-name>`.
    pub root: PathBuf,
    /// Hash stored in `.symskills.json`.
    pub source_hash: String,
    /// Every materialized file in lexical order.
    pub files: Vec<MaterializedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OutputEntry {
    path: String,
    kind: String,
    mode: String,
    size: u64,
    sha256: String,
}

/// Writes a rendered skill below `output_root` using directory capabilities.
///
/// The complete tree is built in a fresh sibling directory, synced, and then
/// swapped into place. The previous tree stays in a sibling backup until the
/// swap and its parent-directory sync succeed, allowing every failed step to
/// restore it. Markers and cache hashes are hints only: reuse requires the
/// expected manifest, digest, and actual tree to agree.
///
/// # Errors
/// Returns an error for destination escapes, unsafe source resources, or I/O
/// failures while replacing the rendered tree.
pub fn materialize(
    bundle: &Bundle,
    rendered: &Rendered,
    output_root: &Path,
) -> Result<Materialized, SkillError> {
    materialize_inner(bundle, rendered, output_root, None)
}

/// Materializes a rendered skill while injecting one deterministic I/O fault.
///
/// This narrow seam is intended for rollback regression tests. Passing `None`
/// is equivalent to [`materialize`]; production callers should use that
/// function instead.
///
/// Supported operation names are `write`, `sync-dir`, `swap-backup`,
/// `swap-install`, and `swap-remove-backup`.
///
/// # Errors
/// Returns the same validation, confinement, metadata, and I/O errors as
/// [`materialize`], plus the requested deterministic injected failure.
pub fn materialize_with_fault(
    bundle: &Bundle,
    rendered: &Rendered,
    output_root: &Path,
    operation: Option<&str>,
) -> Result<Materialized, SkillError> {
    materialize_inner(bundle, rendered, output_root, operation)
}

#[allow(clippy::too_many_lines)]
fn materialize_inner(
    bundle: &Bundle,
    rendered: &Rendered,
    output_root: &Path,
    fault_operation: Option<&str>,
) -> Result<Materialized, SkillError> {
    let destination = open_root(output_root)
        .map_err(|error| SkillError(format!("materialize: open destination root: {error}")))?;
    validate_component(&rendered.target, "target")?;
    validate_component(&rendered.name, "skill name")?;
    let relative = PathBuf::from(&rendered.target).join(&rendered.name);
    let parent = relative.parent().unwrap_or_else(|| Path::new("."));
    ensure_dir(&destination, parent)
        .map_err(|error| SkillError(format!("materialize: ensure destination parent: {error}")))?;
    let _destination_lock =
        lock_destination(&destination, parent, &relative).map_err(|error| io_error(&error))?;
    if let Ok(metadata) = destination.symlink_metadata(&relative) {
        if metadata.file_type().is_symlink() {
            return Err(SkillError(
                "destination skill directory is a symlink".into(),
            ));
        }
        if !metadata.is_dir() {
            return Err(SkillError(
                "destination skill path is not a directory".into(),
            ));
        }
    }

    let hash = source_hash(bundle, rendered)?;
    let marker_path = relative.join(".symskills.json");
    let existing_marker = read_marker(&destination, &marker_path)?;
    let stage = make_sibling_dir(&destination, parent, ".symskills-stage-")
        .map_err(|error| io_error(&error))?;
    let mut stage_live = true;

    let result = (|| {
        materialize_stage(bundle, rendered, &destination, &stage, fault_operation)?;
        let expected = collect_manifest(&destination, &stage)?;
        let digest = manifest_digest(&expected)?;
        if marker_matches(existing_marker.as_ref(), &hash, &expected, &digest) {
            match collect_manifest(&destination, &relative) {
                Ok(actual) if actual == expected => {
                    return collect_existing(&destination, &relative, output_root, hash.clone());
                }
                Ok(_) | Err(_) => {}
            }
        }

        let marker = marker_bytes(&hash, existing_marker.clone(), &expected, &digest)?;
        bound_output_with_marker(&expected, &marker)?;
        write_file(
            &destination,
            &stage.join(".symskills.json"),
            &marker,
            0o644,
            fault_operation,
        )
        .map_err(|error| io_error(&error))?;
        sync_tree(&destination, &stage, fault_operation).map_err(|error| io_error(&error))?;

        let old_exists = destination.symlink_metadata(&relative).is_ok();
        let backup = if old_exists {
            Some(
                unique_sibling_name(&destination, parent, ".symskills-backup-")
                    .map_err(|error| io_error(&error))?,
            )
        } else {
            None
        };
        if let Some(backup) = &backup {
            fault(fault_operation, "swap-backup").map_err(|error| io_error(&error))?;
            destination
                .rename(&relative, &destination, backup)
                .map_err(|error| io_error(&error))?;
            if let Err(error) = sync_dir(&destination, parent, fault_operation) {
                return Err(rollback_swap(
                    &destination,
                    &stage,
                    &relative,
                    Some(backup.as_path()),
                    old_exists,
                    &error,
                    parent,
                ));
            }
        }
        if let Err(error) = fault(fault_operation, "swap-install")
            .and_then(|()| destination.rename(&stage, &destination, &relative))
        {
            return Err(rollback_swap(
                &destination,
                &stage,
                &relative,
                backup.as_deref(),
                old_exists,
                &error,
                parent,
            ));
        }
        stage_live = false;
        if let Err(error) = sync_dir(&destination, parent, fault_operation) {
            return Err(rollback_swap(
                &destination,
                &relative,
                &relative,
                backup.as_deref(),
                old_exists,
                &error,
                parent,
            ));
        }
        if let Some(backup) = backup {
            let cleanup = fault(fault_operation, "swap-remove-backup")
                .and_then(|()| destination.remove_dir_all(&backup));
            if let Err(error) = cleanup {
                return Err(rollback_swap(
                    &destination,
                    &relative,
                    &relative,
                    Some(&backup),
                    old_exists,
                    &error,
                    parent,
                ));
            }
        }
        collect_existing(&destination, &relative, output_root, hash)
    })();

    let cleanup_error = if stage_live {
        destination
            .remove_dir_all(&stage)
            .err()
            .map(|error| io_error(&error))
    } else {
        None
    };
    match (result, cleanup_error) {
        (Ok(value), None) => Ok(value),
        (Err(error), None) => Err(error),
        (Ok(_), Some(cleanup)) => Err(SkillError(format!(
            "materialize cleanup staging tree: {cleanup}"
        ))),
        (Err(error), Some(cleanup)) => Err(SkillError(format!(
            "{error}; materialize cleanup staging tree: {cleanup}"
        ))),
    }
}

include!("materialize_stage.rs");
include!("materialize_manifest.rs");
include!("materialize_fs.rs");
