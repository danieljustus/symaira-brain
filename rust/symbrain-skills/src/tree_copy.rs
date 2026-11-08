//! Confined directory-tree copy shared by skill import and pull staging.
//!
//! Symlinks are followed only when they resolve inside the source root, so a
//! bundle cannot smuggle files from elsewhere into the library. Version
//! control metadata (`.git`) and caller-chosen top-level names are skipped.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::SkillError;

const MAX_DEPTH: usize = 64;

/// Copies `src` to `dst`, omitting `.git` directories and any top-level entry
/// named in `skip_top`.
///
/// # Errors
/// Returns I/O failures and symlinks that escape the source root.
pub(crate) fn copy_tree(src: &Path, dst: &Path, skip_top: &[&str]) -> Result<(), SkillError> {
    let root = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    copy_dir(src, dst, &root, skip_top, 0)
}

fn io(context: &str, path: &Path, error: &std::io::Error) -> SkillError {
    SkillError(format!("{context} {}: {error}", path.display()))
}

fn copy_dir(
    src: &Path,
    dst: &Path,
    root: &Path,
    skip_top: &[&str],
    depth: usize,
) -> Result<(), SkillError> {
    if depth > MAX_DEPTH {
        return Err(SkillError(format!(
            "skill tree {} exceeds maximum directory depth",
            src.display()
        )));
    }
    fs::create_dir_all(dst).map_err(|error| io("create", dst, &error))?;
    let mut entries = fs::read_dir(src)
        .map_err(|error| io("read", src, &error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io("read", src, &error))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|error| io("stat", &entry.path(), &error))?;
        if kind.is_dir() && name_text == ".git" {
            continue;
        }
        if depth == 0 && skip_top.iter().any(|skip| *skip == name_text) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let (source, kind) = if kind.is_symlink() {
            let real = confined(&from, root)?;
            let meta = fs::metadata(&real).map_err(|error| io("stat", &real, &error))?;
            (real, meta.file_type())
        } else {
            (from, kind)
        };
        if kind.is_dir() {
            copy_dir(&source, &to, root, skip_top, depth + 1)?;
        } else if kind.is_file() {
            fs::copy(&source, &to).map_err(|error| io("copy", &source, &error))?;
        } else {
            return Err(SkillError(format!(
                "skill tree contains special file {}",
                source.display()
            )));
        }
    }
    Ok(())
}

fn confined(link: &Path, root: &Path) -> Result<PathBuf, SkillError> {
    let real = fs::canonicalize(link).map_err(|error| io("resolve symlink", link, &error))?;
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(SkillError(format!(
            "symlink {} escapes source root {}: points to {}",
            link.display(),
            root.display(),
            real.display()
        )))
    }
}

/// Replaces every entry of `dst` except `.git` with the contents of `fresh`.
///
/// # Errors
/// Returns I/O failures; `fresh` and `dst` must share a filesystem.
pub(crate) fn replace_contents(fresh: &Path, dst: &Path) -> Result<(), SkillError> {
    for entry in fs::read_dir(dst).map_err(|error| io("read", dst, &error))? {
        let entry = entry.map_err(|error| io("read", dst, &error))?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        let result = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|error| io("remove", &path, &error))?;
    }
    for entry in fs::read_dir(fresh).map_err(|error| io("read", fresh, &error))? {
        let entry = entry.map_err(|error| io("read", fresh, &error))?;
        let to = dst.join(entry.file_name());
        fs::rename(entry.path(), &to).map_err(|error| io("move", &to, &error))?;
    }
    Ok(())
}
