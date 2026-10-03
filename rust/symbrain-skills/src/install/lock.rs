//! Collision-safe per-destination and per-base install locks.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::replace::{open_existing_dir, open_trusted_dir};
use crate::model::SkillError;

const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(10);

/// An ordered set of advisory locks held for the duration of one install.
pub(crate) struct InstallLocks {
    _files: Vec<File>,
}

/// Locks all paths in lexical order, preventing destination/base deadlocks.
pub(crate) fn acquire(paths: &[PathBuf]) -> Result<InstallLocks, SkillError> {
    acquire_with_timeout(paths, LOCK_WAIT)
}

fn acquire_with_timeout(paths: &[PathBuf], timeout: Duration) -> Result<InstallLocks, SkillError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let deadline = Instant::now() + timeout;
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
            .map_err(|error| {
                SkillError(format!(
                    "open install lock {}: {error}",
                    path.display()
                ))
            })?
            .into_std();
        lock_file(&file, &path, true, deadline)?;
        files.push(file);
    }
    Ok(InstallLocks { _files: files })
}

/// Shares locks for existing paths without creating roots or lock files.
/// Read-only status scans use this to avoid observing an install's directory
/// transaction halfway through while retaining their no-write contract.
pub(crate) fn acquire_existing_shared(paths: &[PathBuf]) -> Result<InstallLocks, SkillError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let deadline = Instant::now() + LOCK_WAIT;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let parent = path
            .parent()
            .ok_or_else(|| SkillError("lock path has no parent".to_owned()))?;
        let root = match open_existing_dir(parent) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(SkillError(format!(
                    "open existing install lock root: {error}"
                )));
            }
        };
        let lock_name = lock_name(&path);
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = match root.open_with(Path::new(&lock_name), &options) {
            Ok(file) => file.into_std(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(SkillError(format!("open existing install lock: {error}"))),
        };
        lock_file(&file, &path, false, deadline)?;
        files.push(file);
    }
    Ok(InstallLocks { _files: files })
}

fn lock_file(
    file: &File,
    path: &Path,
    exclusive: bool,
    deadline: Instant,
) -> Result<(), SkillError> {
    loop {
        let result = if exclusive {
            FileExt::try_lock_exclusive(file)
        } else {
            FileExt::try_lock_shared(file)
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(LOCK_POLL);
            }
            Err(error) => {
                return Err(SkillError(format!(
                    "lock install path {}: {error}",
                    path.display()
                )));
            }
        }
    }
}

fn lock_name(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!(".symskills-lock-{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_releases_locks_acquired_earlier_in_the_set() {
        let root = tempfile::tempdir().expect("test root");
        let first = root.path().join("first");
        let held_path = root.path().join("held");
        let held = acquire(std::slice::from_ref(&held_path)).expect("hold second lock");

        let error = acquire_with_timeout(&[first.clone(), held_path], Duration::from_millis(30))
            .err()
            .expect("contended lock times out");
        assert!(error.0.contains("lock install path"));

        let released = acquire(std::slice::from_ref(&first))
            .expect("first lock is released when later acquisition fails");
        drop(released);
        drop(held);
    }
}
