//! Resolution of user-configured skill roots that are themselves symlinks.
//!
//! The capability walk in `replace` opens every path component without
//! following symlinks, which defends the managed tree against link swaps. A
//! user may still point a target's skills root (for example `~/.claude/skills`)
//! at another directory on purpose. Roots handed out by
//! [`crate::target::skill_root`] are registered here; when a walk reaches a
//! registered root that is a symlink, the root is canonicalized once, required
//! to be a directory owned by the current user, and the remaining components
//! below it are still opened without following symlinks.

use std::collections::BTreeSet;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ROOTS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Records a configured skill root so later walks may resolve a symlinked root.
pub(crate) fn register(root: &Path) {
    let Ok(absolute) = std::path::absolute(root) else {
        return;
    };
    if let Ok(mut roots) = ROOTS.lock() {
        roots.insert(absolute);
    }
}

fn containing_root(absolute: &Path) -> Option<PathBuf> {
    let roots = ROOTS.lock().ok()?;
    roots
        .iter()
        .filter(|root| absolute.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
}

/// Rewrites the registered-root prefix of `absolute` to the root's canonical
/// location when that root is a symlink. Paths outside any registered root, or
/// below a root that is not a symlink, are returned unchanged so the strict
/// no-follow walk applies to them exactly as before.
pub(crate) fn resolve(absolute: &Path) -> io::Result<PathBuf> {
    let Some(root) = containing_root(absolute) else {
        return Ok(absolute.to_path_buf());
    };
    let is_link = match std::fs::symlink_metadata(&root) {
        Ok(metadata) => metadata.file_type().is_symlink(),
        // An absent or unreadable root is reported by the walk itself.
        Err(_) => false,
    };
    if !is_link {
        return Ok(absolute.to_path_buf());
    }
    let canonical = std::fs::canonicalize(&root)?;
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_dir() {
        return Err(io::Error::from(io::ErrorKind::NotADirectory));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "symlinked skill root is not owned by the current user",
        ));
    }
    let below = absolute
        .strip_prefix(&root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(canonical.join(below))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::register;
    use crate::install::replace::open_existing_dir;

    #[test]
    fn walk_follows_only_the_root_symlink_not_links_below_it() {
        let dir = tempfile::tempdir().expect("disposable root");
        let real = dir.path().join("real");
        let outside = dir.path().join("outside");
        fs::create_dir_all(real.join("managed")).expect("real root");
        fs::create_dir_all(&outside).expect("outside");
        let root = dir.path().join("skills");
        symlink(&real, &root).expect("symlinked root");
        symlink(&outside, real.join("swapped")).expect("link below root");
        register(&root);

        open_existing_dir(&root).expect("root itself resolves");
        open_existing_dir(&root.join("managed")).expect("real child resolves");
        let error = open_existing_dir(&root.join("swapped")).expect_err("below-root link");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
        let error = open_existing_dir(&root.join("swapped/deeper")).expect_err("below-root link");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[test]
    fn unregistered_symlink_directories_stay_rejected() {
        let dir = tempfile::tempdir().expect("disposable root");
        let real = dir.path().join("real");
        fs::create_dir_all(&real).expect("real");
        let link = dir.path().join("link");
        symlink(&real, &link).expect("link");
        let error = open_existing_dir(&link).expect_err("unregistered link");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }
}
