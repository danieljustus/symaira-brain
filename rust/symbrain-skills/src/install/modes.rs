//! Executable-bit policy for rendered skill trees.

use std::path::Path;

use cap_std::fs::Dir;
#[cfg(unix)]
use cap_std::fs::{Permissions, PermissionsExt};

use super::ModeChange;
use super::replace::open_trusted_dir;
use crate::model::SkillError;

pub(crate) fn executable_changes(root: &Path, allow: bool) -> Result<Vec<ModeChange>, SkillError> {
    if allow {
        return Ok(Vec::new());
    }
    let trusted = open_trusted_dir(root)?;
    let mut changes = Vec::new();
    collect_modes(&trusted, Path::new("."), Path::new("."), &mut changes)?;
    Ok(changes)
}

pub(crate) fn strip_executable_bits(root: &Path) -> Result<(), SkillError> {
    let trusted = open_trusted_dir(root)?;
    strip_modes(&trusted, Path::new("."))
}

fn collect_modes(
    root: &Dir,
    current: &Path,
    relative: &Path,
    changes: &mut Vec<ModeChange>,
) -> Result<(), SkillError> {
    let entries = read_entries(root, current)?;
    for entry in entries {
        let name = entry.file_name();
        let path = current.join(&name);
        let rel = relative.join(&name);
        let metadata = root
            .symlink_metadata(&path)
            .map_err(|error| SkillError(format!("stat install source: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillError("install source contains a symlink".to_owned()));
        }
        if metadata.is_dir() {
            collect_modes(root, &path, &rel, changes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(SkillError(
                "install source contains a special file".to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o111 != 0 {
                changes.push(ModeChange {
                    path: rel.to_string_lossy().replace('\\', "/"),
                    from: format!("{mode:04o}"),
                    to: format!("{:04o}", mode & !0o111),
                });
            }
        }
    }
    Ok(())
}

fn strip_modes(root: &Dir, current: &Path) -> Result<(), SkillError> {
    let entries = read_entries(root, current)?;
    for entry in entries {
        let name = entry.file_name();
        let path = current.join(&name);
        let metadata = root
            .symlink_metadata(&path)
            .map_err(|error| SkillError(format!("stat install source: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillError("install source contains a symlink".to_owned()));
        }
        if metadata.is_dir() {
            strip_modes(root, &path)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(SkillError(
                "install source contains a special file".to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o111 != 0 {
                root.set_permissions(&path, Permissions::from_mode(mode & !0o111))
                    .map_err(|error| SkillError(format!("strip executable bits: {error}")))?;
            }
        }
    }
    Ok(())
}

fn read_entries(root: &Dir, current: &Path) -> Result<Vec<cap_std::fs::DirEntry>, SkillError> {
    let mut entries = root
        .read_dir(current)
        .map_err(|error| SkillError(format!("read install source: {error}")))?
        .map(|entry| {
            entry.map_err(|error| SkillError(format!("read install source entry: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    Ok(entries)
}
