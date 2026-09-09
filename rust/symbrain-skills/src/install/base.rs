//! Separately atomic base snapshots used by three-way status classification.
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::map_unwrap_or,
    clippy::must_use_candidate,
    clippy::needless_return,
    clippy::collapsible_if
)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
#[cfg(unix)]
use cap_std::fs::PermissionsExt;
use serde::{Deserialize, Serialize};

use super::destination::entry_metadata;
use super::drift::file_hashes;
use super::replace::{FaultPoint, open_trusted_dir, unique_name};
use crate::model::SkillError;

/// Base snapshot manifest schema version.
pub const BASE_SCHEMA_VERSION: u32 = 1;
/// The base manifest filename.
pub const BASE_MANIFEST: &str = "manifest.json";

/// A frozen file digest and permission mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseFile {
    /// SHA-256 of the file bytes.
    pub sha256: String,
    /// Unix permission bits in octal form.
    pub mode: String,
}

/// The manifest stored beside a frozen installed tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Target identity.
    pub target: String,
    /// Skill name.
    pub name: String,
    /// Stable identity of the project root for project-scope snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Files keyed by slash-separated relative path.
    pub files: BTreeMap<String, BaseFile>,
}

/// Returns the default OpenCode user-scope base snapshot path.
pub fn base_path(
    home: &Path,
    custom_base: Option<&Path>,
    name: &str,
) -> Result<PathBuf, SkillError> {
    base_path_for(home, custom_base, "opencode", "user", name)
}

/// Returns a base snapshot path for any registered target and scope.
pub fn base_path_for(
    home: &Path,
    custom_base: Option<&Path>,
    target: &str,
    scope: &str,
    name: &str,
) -> Result<PathBuf, SkillError> {
    base_path_for_scope(home, custom_base, target, scope, name, None)
}

/// Returns a scope-aware base path. Project snapshots are namespaced by a
/// stable hash of the project root so two projects cannot overwrite one
/// another's snapshots.
pub fn base_path_for_scope(
    home: &Path,
    custom_base: Option<&Path>,
    target: &str,
    scope: &str,
    name: &str,
    project: Option<&Path>,
) -> Result<PathBuf, SkillError> {
    validate_name(name)?;
    if crate::target::lookup(target).is_none() {
        return Err(SkillError(format!("unknown target {target:?}")));
    }
    let root = custom_base.map_or_else(
        || {
            if home.as_os_str().is_empty() {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.to_path_buf())
                    .join(".local/share/symskills/base")
            } else {
                home.join(".local/share/symskills/base")
            }
        },
        Path::to_path_buf,
    );
    if scope == "project" {
        if let Some(project) = project.filter(|path| !path.as_os_str().is_empty()) {
            return Ok(root
                .join(target)
                .join("project")
                .join(project_id(project)?)
                .join(name));
        }
        // Old callers had no project identity. Keep their path readable so
        // existing snapshots remain compatible during migration.
        return Ok(root.join(target).join("project").join(name));
    }
    Ok(root.join(target).join(name))
}

/// Returns the pre-project-identity path used by schema version one.
pub fn legacy_project_base_path(
    home: &Path,
    custom_base: Option<&Path>,
    target: &str,
    name: &str,
) -> Result<PathBuf, SkillError> {
    base_path_for_scope(home, custom_base, target, "project", name, None)
}

fn project_id(project: &Path) -> Result<String, SkillError> {
    use sha2::{Digest, Sha256};
    let absolute = std::path::absolute(project)
        .map_err(|error| SkillError(format!("resolve project identity: {error}")))?;
    let digest = Sha256::digest(absolute.to_string_lossy().as_bytes());
    Ok(format!("{digest:x}"))
}

/// Writes the default OpenCode user-scope snapshot.
pub fn write_snapshot(
    source: &Path,
    destination: &Path,
    name: &str,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    write_snapshot_for_scope(source, destination, "opencode", name, None, fault)
}

/// Writes a snapshot with explicit target identity.
pub fn write_snapshot_for(
    source: &Path,
    destination: &Path,
    target: &str,
    name: &str,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    write_snapshot_for_scope(source, destination, target, name, None, fault)
}

/// Writes a snapshot with explicit target and project identity.
pub fn write_snapshot_for_scope(
    source: &Path,
    destination: &Path,
    target: &str,
    name: &str,
    project: Option<&Path>,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    validate_name(name)?;
    let parent = destination
        .parent()
        .ok_or_else(|| SkillError("base destination has no parent".to_owned()))?;
    let root = open_trusted_dir(parent)?;
    let stage_name = unique_name(".symskills-base-stage-")?
        .to_string_lossy()
        .into_owned();
    let stage_path = parent.join(&stage_name);
    root.create_dir(&stage_name)
        .map_err(|error| SkillError(format!("create base staging directory: {error}")))?;
    let result = (|| {
        super::replace::copy_tree_for_base(source, &root, Path::new(&stage_name), fault)?;
        let stage_root = root
            .open_dir_nofollow(&stage_name)
            .map_err(|error| SkillError(format!("open base staging directory: {error}")))?;
        let remove_marker = stage_root.remove_file(super::marker::MARKER_FILE);
        if let Err(error) = remove_marker {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(SkillError(format!("remove base marker: {error}")));
            }
        }
        let files = file_hashes(&stage_path, false)?;
        let mut manifest_files = BTreeMap::new();
        for (path, sha256) in files {
            let metadata = stage_root
                .symlink_metadata(Path::new(&path))
                .map_err(|error| SkillError(format!("stat base file {path}: {error}")))?;
            manifest_files.insert(
                path,
                BaseFile {
                    sha256,
                    mode: file_mode(&metadata),
                },
            );
        }
        let manifest = BaseManifest {
            schema_version: BASE_SCHEMA_VERSION,
            target: target.to_owned(),
            name: name.to_owned(),
            project_id: project.map(project_id).transpose()?,
            files: manifest_files,
        };
        let bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| SkillError(format!("encode base manifest: {error}")))?;
        let path = Path::new(&stage_name).join(BASE_MANIFEST);
        super::replace::write_bytes(
            &root,
            &path,
            &[bytes, b"\n".to_vec()].concat(),
            0o644,
            fault,
        )?;
        root.open_dir_nofollow(&stage_name)
            .map_err(|error| SkillError(format!("open base stage: {error}")))?
            .into_std_file()
            .sync_all()
            .map_err(|error| SkillError(format!("sync base stage: {error}")))?;
        publish_base(
            &root,
            &stage_name,
            destination.file_name().unwrap_or_default(),
            fault,
        )
    })();
    if root.symlink_metadata(&stage_name).is_ok() {
        let _ = root.remove_dir_all(&stage_name);
    }
    result
}

fn publish_base(
    root: &cap_std::fs::Dir,
    stage: &str,
    destination: &std::ffi::OsStr,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    if fault == Some(FaultPoint::Install) {
        return Err(SkillError("injected base promotion fault".to_owned()));
    }
    let backup = format!(".symskills-base-backup-{}", std::process::id());
    let destination = Path::new(destination);
    let had_old = root.symlink_metadata(destination).is_ok();
    if had_old {
        root.rename(destination, root, Path::new(&backup))
            .map_err(|error| SkillError(format!("move previous base aside: {error}")))?;
    }
    if let Err(error) = root.rename(Path::new(stage), root, destination) {
        if had_old {
            let _ = root.rename(Path::new(&backup), root, destination);
        }
        return Err(SkillError(format!("publish base snapshot: {error}")));
    }
    if let Some(FaultPoint::RemoveBackup) = fault {
        let _ = root.remove_dir_all(destination);
        if had_old {
            let _ = root.rename(Path::new(&backup), root, destination);
        }
        return Err(SkillError("injected base cleanup fault".to_owned()));
    }
    if !had_old {
        return Ok(());
    }
    if let Err(error) = root.remove_dir_all(&backup) {
        let _ = root.remove_dir_all(destination);
        let _ = root.rename(Path::new(&backup), root, destination);
        return Err(SkillError(format!("remove base backup: {error}")));
    }
    Ok(())
}

/// Loads a base manifest, returning `None` when no managed snapshot exists.
pub fn read_manifest(path: &Path) -> Result<Option<BaseManifest>, SkillError> {
    let Some(metadata) = entry_metadata(path)? else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SkillError(
            "base snapshot is not a regular directory".to_owned(),
        ));
    }
    let root = open_trusted_dir(path)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match root.open_with(BASE_MANIFEST, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SkillError(format!("read base manifest: {error}"))),
    };
    if file
        .metadata()
        .map_err(|error| SkillError(format!("stat base manifest: {error}")))?
        .len()
        > crate::model::MAX_INPUT_SIZE
    {
        return Err(SkillError(
            "base manifest exceeds maximum input size".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(crate::model::MAX_INPUT_SIZE.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SkillError(format!("read base manifest: {error}")))?;
    if bytes.len() as u64 > crate::model::MAX_INPUT_SIZE {
        return Err(SkillError(
            "base manifest exceeds maximum input size".to_owned(),
        ));
    }
    let manifest: BaseManifest = serde_json::from_slice(&bytes)
        .map_err(|error| SkillError(format!("parse base manifest: {error}")))?;
    if manifest.schema_version > BASE_SCHEMA_VERSION {
        return Err(SkillError(format!(
            "base manifest schema_version {} is newer than supported version {BASE_SCHEMA_VERSION}",
            manifest.schema_version
        )));
    }
    Ok(Some(manifest))
}

/// Loads a project-aware base manifest, falling back to the schema-one
/// shared project path for backward compatibility.
pub fn read_manifest_for(
    home: &Path,
    custom_base: Option<&Path>,
    target: &str,
    scope: &str,
    name: &str,
    project: Option<&Path>,
) -> Result<Option<BaseManifest>, SkillError> {
    let current = base_path_for_scope(home, custom_base, target, scope, name, project)?;
    if let Some(manifest) = read_manifest(&current)? {
        return Ok(Some(manifest));
    }
    if scope == "project" && project.is_some() {
        return read_manifest(&legacy_project_base_path(home, custom_base, target, name)?);
    }
    Ok(None)
}

/// Converts a manifest to the digest map consumed by drift classification.
pub fn manifest_hashes(manifest: &BaseManifest) -> BTreeMap<String, String> {
    manifest
        .files
        .iter()
        .map(|(path, file)| (path.clone(), file.sha256.clone()))
        .collect()
}

fn validate_name(name: &str) -> Result<(), SkillError> {
    crate::model::validate_skill_name(name)
}

fn file_mode(metadata: &cap_std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        return format!("{:04o}", metadata.permissions().mode() & 0o777);
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        "0644".to_owned()
    }
}
