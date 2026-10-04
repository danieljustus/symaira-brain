//! Bounded revision extraction and staged restore while retaining .git.
use super::run;
use crate::{MAX_INPUT_SIZE, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError};
use cap_std::fs::Dir;
use std::io::Read;
use std::path::{Component, Path};

fn local(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}
/// Extracts only confined regular files, directories and relative links.
/// # Errors
/// Rejects traversal, excessive counts/sizes and failed git reads.
pub fn extract(dir: &Path, rev: &str, dst: &Path) -> Result<(), SkillError> {
    std::fs::create_dir_all(dst).map_err(|error| SkillError(error.to_string()))?;
    let root = Dir::open_ambient_dir(dst, ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let data = run(dir, &["archive", "--format=tar", rev])?;
    let mut archive = tar::Archive::new(data.as_slice());
    let entries = archive
        .entries()
        .map_err(|error| SkillError(error.to_string()))?;
    let mut total = 0_u64;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RESOURCE_ENTRIES {
            return Err(SkillError("revision archive exceeds entry limit".into()));
        }
        let mut entry = entry.map_err(|error| SkillError(error.to_string()))?;
        let path = entry
            .path()
            .map_err(|error| SkillError(error.to_string()))?
            .into_owned();
        if !local(&path)
            || path
                .components()
                .any(|component| component.as_os_str() == ".git")
        {
            return Err(SkillError(format!(
                "archive entry {:?} escapes destination",
                path
            )));
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            root.create_dir_all(&path)
                .map_err(|error| SkillError(error.to_string()))?;
            continue;
        }
        root.create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))
            .map_err(|error| SkillError(error.to_string()))?;
        if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(|error| SkillError(error.to_string()))?
                .ok_or_else(|| SkillError("archive symlink target missing".into()))?;
            if target.is_absolute() || !confined_link(&path, &target) {
                return Err(SkillError(format!(
                    "archive symlink {:?} escapes destination",
                    path
                )));
            }
            #[cfg(unix)]
            root.symlink(&target, &path)
                .map_err(|error| SkillError(error.to_string()))?;
            #[cfg(windows)]
            root.symlink_file(&target, &path)
                .map_err(|error| SkillError(error.to_string()))?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| SkillError("revision archive byte limit overflow".into()))?;
        if size > MAX_INPUT_SIZE || total > MAX_TOTAL_RESOURCE_BYTES {
            return Err(SkillError("revision archive exceeds byte limit".into()));
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| SkillError(error.to_string()))?;
        if bytes.len() as u64 != size {
            return Err(SkillError("revision archive entry size changed".into()));
        }
        root.write(&path, &bytes)
            .map_err(|error| SkillError(error.to_string()))?;
        #[cfg(unix)]
        {
            use cap_std::fs::PermissionsExt;
            let mode = entry
                .header()
                .mode()
                .map_err(|error| SkillError(error.to_string()))?
                & 0o777;
            root.set_permissions(&path, cap_std::fs::Permissions::from_mode(mode))
                .map_err(|error| SkillError(error.to_string()))?;
        }
    }
    Ok(())
}

/// Replaces the working files from a validated snapshot, preserving .git.
/// # Errors
/// Returns publication or forward-commit failures. A failed commit leaves
/// the restored working files, matching Go; it never claims rollback of git.
pub fn restore(dir: &Path, src: &Path, message: &str) -> Result<String, SkillError> {
    let root = Dir::open_ambient_dir(dir, ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let source = Dir::open_ambient_dir(src, ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let backup = tempfile::tempdir_in(dir.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|error| SkillError(error.to_string()))?;
    let backup_cap = Dir::open_ambient_dir(backup.path(), ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let old = crate::library::library_paths(dir)?;
    let new = crate::library::library_paths(src)?;
    let mut moved_old = Vec::new();
    let mut moved_new = Vec::new();
    let result = (|| {
        for path in old {
            let name = path
                .file_name()
                .ok_or_else(|| SkillError("restore entry filename missing".into()))?;
            if name == ".git" {
                continue;
            }
            root.rename(name, &backup_cap, name)
                .map_err(|error| SkillError(error.to_string()))?;
            moved_old.push(name.to_os_string());
        }
        for path in new {
            let name = path
                .file_name()
                .ok_or_else(|| SkillError("restore entry filename missing".into()))?;
            if name == ".git" {
                return Err(SkillError("restored tree contains .git".into()));
            }
            source
                .rename(name, &root, name)
                .map_err(|error| SkillError(error.to_string()))?;
            moved_new.push(name.to_os_string());
        }
        Ok(())
    })();
    if let Err(error) = result {
        for name in moved_new.into_iter().rev() {
            let _ = root.rename(&name, &source, &name);
        }
        for name in moved_old.into_iter().rev() {
            let _ = backup_cap.rename(&name, &root, &name);
        }
        return Err(error);
    }
    super::commit(dir, message)
}

fn confined_link(path: &Path, target: &Path) -> bool {
    let mut depth = path.parent().map_or(0, |parent| {
        parent
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .count()
    });
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    !target.as_os_str().is_empty()
}
