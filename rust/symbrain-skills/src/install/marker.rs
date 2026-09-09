//! Marker parsing and schema-preserving serialization for installed skills.
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_truncation
)]

use std::io::Read;
use std::path::Path;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{MAX_INPUT_SIZE, SkillError};

/// The marker file inside a managed OpenCode skill directory.
pub const MARKER_FILE: &str = ".symskills.json";
/// The marker schema understood and written by this crate.
pub const MARKER_SCHEMA_VERSION: u32 = 1;

/// The stable fields written by the Go and Rust installers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// Marker schema version. Missing legacy values are read as version one.
    #[serde(default = "default_schema", rename = "schema_version")]
    pub schema_version: u32,
    /// Owning implementation identifier.
    #[serde(default)]
    pub managed_by: String,
    /// Harness target.
    #[serde(default)]
    pub target: String,
    /// Installed skill name.
    #[serde(default)]
    pub name: String,
    /// Render source identity/path retained for diagnostics.
    #[serde(default)]
    pub rendered_at: String,
    /// Installation mode; Phase 7.3A writes `copy` only.
    #[serde(default)]
    pub mode: String,
    /// RFC3339 installation timestamp.
    #[serde(default)]
    pub installed: String,
    /// Render source hash, when available.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_hash: String,
    /// Whether executable resource bits were retained.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_executable: bool,
    /// Unknown additive fields retained when a marker is read and rewritten.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

fn default_schema() -> u32 {
    MARKER_SCHEMA_VERSION
}
fn is_false(value: &bool) -> bool {
    !*value
}

/// Result of reading an install marker without conflating absence and damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerState {
    /// No marker exists.
    Missing,
    /// A valid marker exists.
    Valid(Marker),
    /// A marker exists but is malformed or not an object.
    Malformed(String),
    /// A valid marker uses a schema newer than this binary understands.
    UnsupportedSchema(u32),
}

/// Builds the marker for a fresh copy installation.
#[must_use]
pub fn new_marker(
    target: &str,
    name: &str,
    rendered_at: &Path,
    source_hash: &str,
    allow_executable: bool,
) -> Marker {
    Marker {
        schema_version: MARKER_SCHEMA_VERSION,
        managed_by: "symskills".to_owned(),
        target: target.to_owned(),
        name: name.to_owned(),
        rendered_at: rendered_at.to_string_lossy().into_owned(),
        mode: "copy".to_owned(),
        installed: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        source_hash: source_hash.to_owned(),
        allow_executable,
        extra: std::collections::BTreeMap::new(),
    }
}

/// Reads a marker through a trusted directory capability.
///
/// The capability is opened without following any path component. The final
/// marker open also uses `FollowSymlinks::No`, including on Windows where a
/// reparse point must never become an install-control file.
pub(crate) fn read_marker_at(root: &Dir) -> Result<MarkerState, SkillError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match root.open_with(Path::new(MARKER_FILE), &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MarkerState::Missing);
        }
        Err(error) => return Ok(MarkerState::Malformed(error.to_string())),
    };
    let metadata = file
        .metadata()
        .map_err(|error| SkillError(format!("read marker metadata: {error}")))?;
    if !metadata.is_file() {
        return Ok(MarkerState::Malformed(
            "marker is not a regular file".to_owned(),
        ));
    }
    if metadata.len() > MAX_INPUT_SIZE {
        return Ok(MarkerState::Malformed(format!(
            "marker exceeds maximum input size of {MAX_INPUT_SIZE} bytes"
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_SIZE.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SkillError(format!("read marker: {error}")))?;
    if bytes.len() as u64 > MAX_INPUT_SIZE {
        return Ok(MarkerState::Malformed(format!(
            "marker exceeds maximum input size of {MAX_INPUT_SIZE} bytes"
        )));
    }
    parse_marker(&bytes)
}

/// Reads and classifies a marker with a trusted, no-follow directory walk.
pub fn read_marker(path: &Path) -> Result<MarkerState, SkillError> {
    let root = super::replace::open_trusted_dir(path)?;
    read_marker_at(&root)
}

/// Parses a marker while preserving the distinction between legacy and newer schemas.
pub fn parse_marker(bytes: &[u8]) -> Result<MarkerState, SkillError> {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return Ok(MarkerState::Malformed(error.to_string())),
    };
    let Some(object) = value.as_object() else {
        return Ok(MarkerState::Malformed(
            "marker must be a JSON object".to_owned(),
        ));
    };
    let schema = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(MARKER_SCHEMA_VERSION));
    if schema > u64::from(MARKER_SCHEMA_VERSION) {
        return Ok(MarkerState::UnsupportedSchema(schema as u32));
    }
    let marker: Marker = match serde_json::from_value(value) {
        Ok(marker) => marker,
        Err(error) => return Ok(MarkerState::Malformed(error.to_string())),
    };
    Ok(MarkerState::Valid(marker))
}

/// Refuses a destination marker that this binary cannot safely rewrite.
pub fn ensure_writable(state: &MarkerState, path: &Path) -> Result<(), SkillError> {
    match state {
        MarkerState::UnsupportedSchema(version) => Err(SkillError(format!(
            "refusing to overwrite marker {}: schema_version {version} is newer than supported version {MARKER_SCHEMA_VERSION}",
            path.display()
        ))),
        MarkerState::Malformed(error) => Err(SkillError(format!(
            "refusing to overwrite malformed marker {}: {error}",
            path.display()
        ))),
        MarkerState::Missing | MarkerState::Valid(_) => Ok(()),
    }
}

/// Serializes a marker deterministically with a trailing newline.
pub fn encode_marker(marker: &Marker) -> Result<Vec<u8>, SkillError> {
    let bytes = serde_json::to_vec_pretty(marker)
        .map_err(|error| SkillError(format!("encode marker: {error}")))?;
    if bytes.len() as u64 >= MAX_INPUT_SIZE {
        return Err(SkillError(
            "encoded marker exceeds maximum input size".to_owned(),
        ));
    }
    let mut output = bytes;
    output.push(b'\n');
    Ok(output)
}

pub(crate) fn marker_bytes(
    target: &str,
    name: &str,
    source: &Path,
    source_hash: &str,
    mode: &str,
    allow: bool,
    previous: Option<Marker>,
) -> Result<Vec<u8>, SkillError> {
    let mut marker = new_marker(target, name, source, source_hash, allow);
    mode.clone_into(&mut marker.mode);
    if let Some(previous) = previous {
        marker.extra = previous.extra;
    }
    encode_marker(&marker)
}
