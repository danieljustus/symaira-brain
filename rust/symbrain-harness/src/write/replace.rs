use cap_std::fs::Dir;
#[cfg(windows)]
use std::fmt::Write as _;
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use super::MAX_WINDOWS_REPLACE_ATTEMPTS;
#[cfg(windows)]
use super::atomic::cleanup_file;

pub(super) fn replace_existing(parent: &Dir, temporary: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        // Windows rename does not replace an existing destination. Move the
        // original to a unique sibling first, then either install the temp
        // file or move that original back. The original is never deleted
        // before the replacement has succeeded.
        let first_error = match parent.rename(temporary, parent, target) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if !matches!(
            first_error.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
        ) {
            return Err(first_error);
        }

        let target_name = target.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "replacement target has no file name",
            )
        })?;
        let mut rollback_path = PathBuf::new();
        let mut moved = false;
        let mut last_move_error = first_error;
        for suffix in 0_u32..MAX_WINDOWS_REPLACE_ATTEMPTS {
            rollback_path = rollback_name(target_name, suffix);
            match parent.rename(target, parent, &rollback_path) {
                Ok(()) => {
                    moved = true;
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // The destination disappeared between attempts; retry
                    // the original replacement rather than deleting anything.
                    match parent.rename(temporary, parent, target) {
                        Ok(()) => return Ok(()),
                        Err(retry_error) => last_move_error = retry_error,
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    last_move_error = error;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
        if !moved {
            return Err(last_move_error);
        }

        let mut replacement_error = None;
        for _ in 0..MAX_WINDOWS_REPLACE_ATTEMPTS {
            match parent.rename(temporary, parent, target) {
                Ok(()) => {
                    // Only now is it safe to remove the rollback copy. A
                    // cleanup failure is returned while leaving that copy for
                    // manual recovery rather than losing the original.
                    return cleanup_file(parent, &rollback_path).map_err(|cleanup| {
                        io::Error::other(format!(
                            "replacement succeeded but rollback cleanup failed: {cleanup}"
                        ))
                    });
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    replacement_error = Some(error);
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    replacement_error = Some(error);
                    break;
                }
            }
        }
        let replacement_error = replacement_error
            .unwrap_or_else(|| io::Error::other("bounded Windows replacement failed"));
        let rollback_error = move_with_retries(parent, &rollback_path, target);
        let cleanup_error = cleanup_file(parent, temporary);
        Err(join_windows_errors(
            &replacement_error,
            rollback_error,
            cleanup_error,
        ))
    }
    #[cfg(not(windows))]
    parent.rename(temporary, parent, target)
}

#[cfg(windows)]
fn rollback_name(base: &std::ffi::OsStr, suffix: u32) -> PathBuf {
    let mut name = base.to_os_string();
    name.push(".symbrain-rollback");
    if suffix != 0 {
        name.push(format!("-{suffix}"));
    }
    PathBuf::from(name)
}

#[cfg(windows)]
fn join_windows_errors(
    replacement: &io::Error,
    rollback: io::Result<()>,
    cleanup: io::Result<()>,
) -> io::Error {
    let mut message = replacement.to_string();
    if let Err(error) = rollback {
        let _ = write!(message, "; rollback failed: {error}");
    }
    if let Err(error) = cleanup {
        let _ = write!(message, "; temporary cleanup failed: {error}");
    }
    io::Error::other(message)
}

#[cfg(windows)]
fn move_with_retries(parent: &Dir, source: &Path, target: &Path) -> io::Result<()> {
    let mut last_error = None;
    for _ in 0..MAX_WINDOWS_REPLACE_ATTEMPTS {
        match parent.rename(source, parent, target) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
                ) =>
            {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("bounded Windows rollback failed")))
}
