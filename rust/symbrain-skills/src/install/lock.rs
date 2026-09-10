//! Collision-safe per-destination and per-base install locks.

use std::fs::File;
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::replace::open_trusted_dir;
use crate::model::SkillError;

/// An ordered set of advisory locks held for the duration of one install.
pub(crate) struct InstallLocks {
    _files: Vec<File>,
}

/// Locks all paths in lexical order, preventing destination/base deadlocks.
pub(crate) fn acquire(paths: &[PathBuf]) -> Result<InstallLocks, SkillError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let parent = path
            .parent()
            .ok_or_else(|| SkillError("lock path has no parent".to_owned()))?;

        let root = open_trusted_dir(parent)?;
        let lock_name = lock_name(&path);
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No);
        let file = root
            .open_with(Path::new(&lock_name), &options)
            .map_err(|error| SkillError(format!("open install lock: {error}")))?
            .into_std();
        file.lock_exclusive().map_err(|error| {
            SkillError(format!("lock install path {}: {error}", path.display()))
        })?;
        files.push(file);
    }
    Ok(InstallLocks { _files: files })
}

fn lock_name(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!(".symskills-lock-{digest:x}")
}
