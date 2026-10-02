//! Collision-safe per-skill and shared-directory install locks.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::replace::{open_existing_dir, open_trusted_dir};
use crate::model::SkillError;

const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(10);

/// An ordered set of advisory locks held for the duration of one operation.
pub(crate) struct InstallLocks {
    _files: Vec<File>,
}

/// Locks all paths exclusively in lexical order, preventing transaction and
/// destination/base deadlocks. Callers include shared parent paths when their
/// operation mutates siblings in one directory.
pub(crate) fn acquire(paths: &[PathBuf]) -> Result<InstallLocks, SkillError> {
    acquire_with_timeout(paths, LOCK_WAIT, false, false)
}

/// Shares existing installer locks without creating files or directories.
/// Legacy roots without lock files remain readable without initializing them.
pub(crate) fn acquire_existing_shared(paths: &[PathBuf]) -> Result<InstallLocks, SkillError> {
    acquire_with_timeout(paths, LOCK_WAIT, true, true)
}

fn acquire_with_timeout(
    paths: &[PathBuf],
    timeout: Duration,
    shared: bool,
    existing_only: bool,
) -> Result<InstallLocks, SkillError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let deadline = Instant::now() + timeout;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let lock_name = lock_name(&path);
        let Some(file) = open_lock_file(&path, &lock_name, existing_only, deadline)? else {
            continue;
        };
        lock_file(&file, &path, shared, deadline)?;
        files.push(file);
    }
    Ok(InstallLocks { _files: files })
}

fn open_lock_file(
    path: &Path,
    lock_name: &str,
    existing_only: bool,
    deadline: Instant,
) -> Result<Option<File>, SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("lock path has no parent".to_owned()))?;
    loop {
        let root = if existing_only {
            match open_existing_dir(parent) {
                Ok(root) => root,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => {
                    return Err(SkillError(format!(
                        "open existing install lock directory {}: {error}",
                        parent.display()
                    )));
                }
            }
        } else {
            open_trusted_dir(parent)?
        };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(!existing_only)
            .create(!existing_only)
            .follow(FollowSymlinks::No);
        // Read-only opening of a FIFO would bypass the contention deadline.
        #[cfg(unix)]
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
        match root.open_with(Path::new(lock_name), &options) {
            Ok(file) => {
                let metadata = file.metadata().map_err(|error| {
                    SkillError(format!("stat install lock {}: {error}", path.display()))
                })?;
                if !metadata.is_file() {
                    return Err(SkillError(format!(
                        "install lock {} must be a regular file",
                        path.display()
                    )));
                }
                return Ok(Some(file.into_std()));
            }
            Err(error) if existing_only && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && Instant::now() < deadline =>
            {
                thread::sleep(LOCK_POLL);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(SkillError(format!(
                    "timed out opening install lock {}",
                    path.display()
                )));
            }
            Err(error) => {
                return Err(SkillError(format!(
                    "open install lock {}: {error}",
                    path.display()
                )));
            }
        }
    }
}

fn lock_file(file: &File, path: &Path, shared: bool, deadline: Instant) -> Result<(), SkillError> {
    loop {
        let result = if shared {
            FileExt::try_lock_shared(file)
        } else {
            FileExt::try_lock_exclusive(file)
        };
        match result {
            Ok(()) => return Ok(()),
            // fs2 uses ERROR_LOCK_VIOLATION on Windows, not WouldBlock.
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(SkillError(format!(
                        "timed out waiting for install lock {}",
                        path.display()
                    )));
                }
                thread::sleep(remaining.min(LOCK_POLL));
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
    use std::fs::{self, OpenOptions as StdOpenOptions};
    use std::time::Duration;

    use fs2::FileExt;
    use tempfile::tempdir;

    use super::{acquire_existing_shared, acquire_with_timeout, lock_name};

    #[cfg(unix)]
    #[test]
    fn special_lock_files_are_rejected_without_blocking() {
        let temp = tempdir().expect("temporary root");
        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let install = root.join("skill");
        let lock_path = root.join(lock_name(&install));
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&lock_path)
                .status()
                .expect("create FIFO fixture")
                .success()
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let errors = [true, false].map(|shared| {
                acquire_with_timeout(
                    std::slice::from_ref(&install),
                    Duration::from_millis(100),
                    shared,
                    shared,
                )
                .err()
                .map(|error| error.0)
            });
            sender.send(errors).expect("send lock rejection results");
        });
        let errors = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("opening a special lock file must not block");
        assert!(errors.iter().all(|error| {
            error
                .as_ref()
                .is_some_and(|message| message.ends_with("must be a regular file"))
        }));
    }

    #[test]
    fn read_locks_do_not_initialize_legacy_roots() {
        let temp = tempdir().expect("temporary root");
        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let install = root.join("existing/skill");
        fs::create_dir(install.parent().expect("existing parent")).expect("legacy root");
        let _locks = acquire_existing_shared(&[install, root.join("absent/skill")])
            .expect("read existing roots without initializing locks");
        assert_eq!(fs::read_dir(&root).expect("root entries").count(), 1);
        assert_eq!(
            fs::read_dir(root.join("existing"))
                .expect("legacy entries")
                .count(),
            0
        );
    }

    #[test]
    fn timeout_releases_locks_acquired_earlier_in_the_set() {
        let temp = tempdir().expect("temporary root");
        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let first = root.join("one/install");
        let second = root.join("two/install");
        let first_parent = first.parent().expect("first parent");
        let second_parent = second.parent().expect("second parent");
        fs::create_dir_all(first_parent).expect("first lock directory");
        fs::create_dir_all(second_parent).expect("second lock directory");

        let second_lock_path = second_parent.join(lock_name(&second));
        let blocker = StdOpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(second_lock_path)
            .expect("open blocking lock");
        FileExt::lock_exclusive(&blocker).expect("hold second lock");

        let result = acquire_with_timeout(
            &[first.clone(), second],
            Duration::from_millis(30),
            false,
            false,
        );
        let error = result.err().expect("second lock must time out");
        assert!(error.0.starts_with("timed out waiting for install lock"));

        let first_lock_path = first_parent.join(lock_name(&first));
        let first_lock = StdOpenOptions::new()
            .read(true)
            .write(true)
            .open(first_lock_path)
            .expect("open first lock for release check");
        FileExt::try_lock_exclusive(&first_lock)
            .expect("first lock is released when later acquisition fails");
    }
}
