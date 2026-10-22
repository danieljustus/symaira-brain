//! Bounded revision extraction and staged restore while retaining `.git`.

use crate::{MAX_INPUT_SIZE, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError};
use cap_std::fs::Dir;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

fn local(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
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

fn err(error: impl std::fmt::Display) -> SkillError {
    SkillError(error.to_string())
}

/// Materializes the tracked tree at `rev` into `dst` through a capability
/// handle: directories, regular files (archive mode bits) and confined
/// relative symlinks only; traversal and oversized archives are refused.
///
/// # Errors
/// Rejects traversal, excessive counts/sizes and failed git reads.
// Escaped, quoted path diagnostics are the Go `%q` presentation contract.
#[allow(clippy::unnecessary_debug_formatting)]
pub fn extract(dir: &Path, rev: &str, dst: &Path) -> Result<(), SkillError> {
    std::fs::create_dir_all(dst).map_err(err)?;
    let root = Dir::open_ambient_dir(dst, ambient_authority::ambient_authority()).map_err(err)?;
    let data = super::archive(dir, rev)?;
    let mut archive = tar::Archive::new(data.as_slice());
    let mut total = 0_u64;
    for (index, entry) in archive.entries().map_err(err)?.enumerate() {
        if index >= MAX_RESOURCE_ENTRIES {
            return Err(SkillError("revision archive exceeds entry limit".into()));
        }
        let mut entry = entry.map_err(err)?;
        let path = entry.path().map_err(err)?.into_owned();
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        if !local(&path) {
            return Err(SkillError(format!(
                "archive entry {path:?} escapes destination"
            )));
        }
        if kind.is_dir() {
            root.create_dir_all(&path).map_err(err)?;
            continue;
        }
        if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(err)?
                .ok_or_else(|| SkillError("archive symlink target missing".into()))?;
            if target.is_absolute() || !confined_link(&path, &target) {
                return Err(SkillError(format!(
                    "archive symlink {path:?} escapes destination"
                )));
            }
            root.create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))
                .map_err(err)?;
            let _ = root.remove_file(&path);
            #[cfg(unix)]
            root.symlink(&target, &path).map_err(err)?;
            #[cfg(windows)]
            root.symlink_file(&target, &path).map_err(err)?;
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
        root.create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))
            .map_err(err)?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(err)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            // Applied at creation like Go, so umask stays authoritative.
            options.mode(entry.header().mode().map_err(err)? & 0o777);
        }
        root.open_with(&path, &options)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(err)?;
    }
    Ok(())
}

/// `fsutil.CopyTree`: symlinks are followed (and must stay inside the source
/// root); `.git` directories are skipped.
fn copy_tree(real_root: &Path, src: &Path, dst: &Path) -> Result<(), SkillError> {
    std::fs::create_dir_all(dst).map_err(err)?;
    let mut entries: Vec<_> = std::fs::read_dir(src)
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let path = entry.path();
        let target = dst.join(&name);
        let kind = entry.file_type().map_err(err)?;
        if kind.is_dir() && name == ".git" {
            continue;
        }
        if kind.is_symlink() {
            let real = std::fs::canonicalize(&path).map_err(err)?;
            if !real.starts_with(real_root) {
                return Err(SkillError(format!(
                    "symlink {} escapes source root {}: points to {}",
                    crate::gotext::quote(&name.to_string_lossy()),
                    crate::gotext::quote(&src.to_string_lossy()),
                    crate::gotext::quote(&real.to_string_lossy())
                )));
            }
            let metadata = std::fs::metadata(&real).map_err(err)?;
            if metadata.is_dir() {
                copy_tree(real_root, &real, &target)?;
            } else {
                copy_file(&real, &target)?;
            }
        } else if kind.is_dir() {
            copy_tree(real_root, &path, &target)?;
        } else {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), SkillError> {
    let mut input = std::fs::File::open(src).map_err(err)?;
    let mode = input.metadata().map_err(err)?.permissions();
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(mode.mode() & 0o777);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut output = options.open(dst).map_err(err)?;
    std::io::copy(&mut input, &mut output).map_err(err)?;
    Ok(())
}

/// Replaces the working files of `dir` with a copy of `src` (keeping `.git`)
/// and records a forward commit. The copy lands in a sibling temporary
/// directory first, so a failed copy never leaves `dir` half-written.
///
/// # Errors
/// Returns copy, publication or forward-commit failures.
pub fn restore(dir: &Path, src: &Path, message: &str) -> Result<String, SkillError> {
    if !super::is_repo(dir) {
        return Err(SkillError(format!(
            "not a git repository: {}",
            dir.display()
        )));
    }
    let parent: PathBuf = dir.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let staging = tempfile::Builder::new()
        .prefix(".restore-tmp-")
        .tempdir_in(&parent)
        .map_err(err)?;
    let real_root = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    copy_tree(&real_root, src, staging.path())?;
    for entry in std::fs::read_dir(dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type().map_err(err)?.is_dir() {
            std::fs::remove_dir_all(&path).map_err(err)?;
        } else {
            std::fs::remove_file(&path).map_err(err)?;
        }
    }
    for entry in std::fs::read_dir(staging.path()).map_err(err)? {
        let entry = entry.map_err(err)?;
        std::fs::rename(entry.path(), dir.join(entry.file_name())).map_err(err)?;
    }
    super::commit(dir, message)
}
