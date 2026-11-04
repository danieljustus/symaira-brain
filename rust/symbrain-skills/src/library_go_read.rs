//! Go-faithful `SKILL.md` read for the library listing.
//!
//! `skill.ListLibrary` opens each bundle with `os.OpenRoot` and reads
//! `SKILL.md` through the root (`root.OpenFile(..., O_NONBLOCK|O_NOFOLLOW)`).
//! `os.Root` resolves symlinks itself: relative links that stay inside the root
//! are followed, absolute links and links leaving the root fail with
//! `path escapes from parent`, and loops end in `ELOOP`. The messages below are
//! the exact texts Go prints, so a damaged bundle reports the same bytes.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::gotext::io_text;

/// `os.Root`'s symlink budget (`rootMaxSymlinks`).
const MAX_SYMLINKS: usize = 255;
const NAME: &str = "SKILL.md";

/// Reads `<root>/SKILL.md`, returning Go's `loadFrontmatterOnly` error text.
pub(crate) fn read_skill_md(root: &Path, limit: u64) -> Result<Vec<u8>, String> {
    drop(std::fs::File::open(root).map_err(|source| {
        format!(
            "open skill root: open {}: {}",
            root.display(),
            io_text(&source)
        )
    })?);
    let resolved =
        resolve(root).map_err(|reason| format!("read {NAME}: openat {NAME}: {reason}"))?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(&resolved)
        .map_err(|source| format!("read {NAME}: openat {NAME}: {}", io_text(&source)))?;
    let metadata = file
        .metadata()
        .map_err(|source| format!("stat {NAME}: {}", io_text(&source)))?;
    if !metadata.is_file() {
        return Err(format!("{NAME} must be a regular file"));
    }
    if metadata.len() > limit {
        return Err(too_large(limit));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| format!("read {NAME}: read {NAME}: {}", io_text(&source)))?;
    if bytes.len() as u64 > limit {
        return Err(too_large(limit));
    }
    Ok(bytes)
}

fn too_large(limit: u64) -> String {
    format!("{NAME} exceeds maximum input size of {limit} bytes")
}

/// Walks `SKILL.md` inside `root` the way `os.Root` does, following relative
/// symlinks only while they stay below the root.
fn resolve(root: &Path) -> Result<PathBuf, String> {
    let mut pending: Vec<OsString> = vec![NAME.into()];
    let mut stack: Vec<OsString> = Vec::new();
    let mut links = 0;
    while let Some(part) = pending.pop() {
        if part == ".." {
            if stack.pop().is_none() {
                return Err("path escapes from parent".to_owned());
            }
            continue;
        }
        if part == "." || part.is_empty() {
            continue;
        }
        let mut candidate = root.to_path_buf();
        candidate.extend(&stack);
        candidate.push(&part);
        let metadata = match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            // A missing final component is reported by the open that follows.
            Err(source) if pending.is_empty() && source.kind() == std::io::ErrorKind::NotFound => {
                stack.push(part);
                break;
            }
            Err(source) => return Err(io_text(&source)),
        };
        if !metadata.file_type().is_symlink() {
            stack.push(part);
            continue;
        }
        links += 1;
        if links > MAX_SYMLINKS {
            return Err("too many levels of symbolic links".to_owned());
        }
        let target = std::fs::read_link(&candidate).map_err(|source| io_text(&source))?;
        if target.is_absolute() {
            return Err("path escapes from parent".to_owned());
        }
        let components: Vec<OsString> = target
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.to_os_string()),
                Component::ParentDir => Some("..".into()),
                _ => None,
            })
            .collect();
        pending.extend(components.into_iter().rev());
    }
    let mut resolved = root.to_path_buf();
    resolved.extend(&stack);
    Ok(resolved)
}
