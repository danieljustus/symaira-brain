#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt;
use cap_std::fs::{Dir, OpenOptions};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::core::AtomicFile;

pub(super) fn reserve_name(parent: &Dir, name: &Path) -> io::Result<bool> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match parent.open_with(name, &options) {
        Ok(file) => {
            drop(file);
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}
pub(super) fn cleanup_file(parent: &Dir, path: &Path) -> io::Result<()> {
    match parent.remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn join_cleanup_error(
    result: io::Result<()>,
    cleanup: io::Result<()>,
    label: &str,
) -> io::Result<()> {
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(io::Error::other(format!("{label} failed: {error}"))),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; {label} failed: {cleanup}"
        ))),
    }
}
/// Atomically writes a path-relative target beneath its parent directory.
///
/// # Errors
/// Returns an I/O error for unsafe targets, failed writes, replacement, or syncing.
pub fn atomic_write(path: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    let file = AtomicFile::open(parent, Path::new(name), path.to_path_buf(), false)?;
    file.write(data, mode)
}

/// Creates a directory tree with restrictive permissions on Unix.
///
/// # Errors
/// Returns an I/O error if a directory cannot be created.
#[allow(dead_code)]
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Computes the first backup destination for a path and timestamp.
#[must_use]
pub fn backup_path(path: &Path, timestamp: &str) -> PathBuf {
    let mut target = path.as_os_str().to_os_string();
    target.push(format!(".bak.{timestamp}"));
    PathBuf::from(target)
}
