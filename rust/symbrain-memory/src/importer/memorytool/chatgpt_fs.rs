//! Symlink- and special-file-safe filesystem access for ChatGPT exports
//! (`chatgpt_secure_unix.go`).
//!
//! Go opened every path component with `O_NOFOLLOW`. The standard library has
//! no `openat`, so this checks each component with `symlink_metadata`, then
//! opens the file and confirms that the opened inode is the one that was
//! checked. A FIFO or device never gets opened (`is_file` is tested first),
//! and a swapped-in symlink is rejected by the identity comparison.

use super::ExportRef;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Largest accepted export: ordinary exports fit, parser memory stays bounded.
pub(super) const MAX_EXPORT_BYTES: u64 = 64 << 20;
const MAX_DISCOVERY_ENTRIES: usize = 10_000;
const MAX_DISCOVERY_DEPTH: usize = 16;
const EXPORT_SUFFIX: &str = "conversations.json";

/// Absolute, lexically cleaned `path`, with macOS's `/var` firmlink spelled out.
fn absolute(path: &Path) -> std::io::Result<PathBuf> {
    let clean = std::path::absolute(path)?;
    let mut out = PathBuf::new();
    for component in clean.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if cfg!(target_os = "macos") && (out == Path::new("/var") || out.starts_with("/var/")) {
        return Ok(Path::new("/private").join(out.strip_prefix("/").unwrap_or(&out)));
    }
    Ok(out)
}

/// Whether any component of `path` (checked from the root down) is a symlink.
/// A missing component is an I/O error.
fn has_symlink(path: &Path) -> std::io::Result<bool> {
    let mut prefix = PathBuf::new();
    for component in absolute(path)?.components() {
        prefix.push(component.as_os_str());
        if std::fs::symlink_metadata(&prefix)?.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reference(path: &Path, meta: &std::fs::Metadata) -> Option<ExportRef> {
    Some(ExportRef {
        tool: "chatgpt".into(),
        path: path.to_string_lossy().into_owned(),
        format: "json".into(),
        modified_at: meta.modified().ok()?.into(),
    })
}

/// Regular `conversations.json` files below `path` (or `path` itself).
/// Symlinks anywhere yield nothing; an unreadable `path` is an error.
pub(super) fn discover(path: &Path) -> Result<Vec<ExportRef>, String> {
    let wrap = |e: std::io::Error| format!("open {}: {e}", path.display());
    if has_symlink(path).map_err(wrap)? {
        return Ok(Vec::new());
    }
    let meta = std::fs::symlink_metadata(path).map_err(wrap)?;
    if meta.is_file() {
        let named = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(EXPORT_SUFFIX));
        return Ok(if named {
            reference(path, &meta).into_iter().collect()
        } else {
            Vec::new()
        });
    }
    let mut refs = Vec::new();
    let mut seen = 0;
    walk(path, 0, &mut seen, &mut refs).map_err(wrap)?;
    Ok(refs)
}

fn walk(
    dir: &Path,
    depth: usize,
    seen: &mut usize,
    refs: &mut Vec<ExportRef>,
) -> std::io::Result<()> {
    let remaining = MAX_DISCOVERY_ENTRIES.saturating_sub(*seen);
    if remaining == 0 {
        return Ok(());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)?.take(remaining) {
        names.push(entry?.file_name());
    }
    *seen += names.len();
    names.sort();
    for name in names {
        let entry = dir.join(&name);
        // symlink_metadata skips symlinks (including ones substituted late).
        let Ok(meta) = std::fs::symlink_metadata(&entry) else {
            continue;
        };
        if meta.is_dir() && depth < MAX_DISCOVERY_DEPTH {
            let _ = walk(&entry, depth + 1, seen, refs);
        } else if meta.is_file() && name.to_string_lossy().ends_with(EXPORT_SUFFIX) {
            refs.extend(reference(&entry, &meta));
        }
    }
    Ok(())
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Reads one export: regular file, no symlink, at most [`MAX_EXPORT_BYTES`].
pub(super) fn read_export(path: &Path) -> Result<Vec<u8>, String> {
    let io = |e: std::io::Error| e.to_string();
    if has_symlink(path).map_err(io)? {
        return Err(format!("{} is a symbolic link", path.display()));
    }
    let checked = std::fs::symlink_metadata(path).map_err(io)?;
    if !checked.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let file = std::fs::File::open(path).map_err(io)?;
    let opened = file.metadata().map_err(io)?;
    if !opened.is_file() || !same_file(&checked, &opened) {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let too_big = || {
        format!(
            "{} exceeds maximum size of {MAX_EXPORT_BYTES} bytes",
            path.display()
        )
    };
    if opened.len() > MAX_EXPORT_BYTES {
        return Err(too_big());
    }
    let mut data = Vec::new();
    file.take(MAX_EXPORT_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(io)?;
    if data.len() as u64 > MAX_EXPORT_BYTES {
        return Err(too_big());
    }
    Ok(data)
}
