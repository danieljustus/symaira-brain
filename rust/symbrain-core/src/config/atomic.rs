//! Atomic filesystem writes and directory creation matching corekit contracts.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tempfile::NamedTempFile;

/// Retries an operation up to 10 attempts with a 10ms backoff on error.
/// Parameterized with `rename_op` and `sleep_op` for testability without real sleeps.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn rename_with_retry_impl<R, S>(mut rename_op: R, mut sleep_op: S) -> std::io::Result<()>
where
    R: FnMut(usize) -> std::io::Result<()>,
    S: FnMut(usize, Duration),
{
    let mut last_err = None;
    for attempt in 0..10 {
        match rename_op(attempt) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                if attempt < 9 {
                    sleep_op(attempt, Duration::from_millis(10));
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("rename retry attempts exhausted")))
}

#[cfg(windows)]
fn atomic_rename(temp_path: &Path, dest_path: &Path) -> std::io::Result<()> {
    rename_with_retry_impl(
        |_| fs::rename(temp_path, dest_path),
        |_, dur| std::thread::sleep(dur),
    )
}

#[cfg(all(not(windows), not(unix)))]
fn atomic_rename(temp_path: &Path, dest_path: &Path) -> std::io::Result<()> {
    fs::rename(temp_path, dest_path)
}

/// Recursively creates parent directories with 0700 permissions on Unix.
pub fn create_dir_all(parent: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(parent)
}

/// Writes `data` atomically to `path` with 0600 permissions on Unix.
/// On Windows, performs sharing-violation retries matching corekit (10 attempts, 10ms backoff).
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = NamedTempFile::new_in(parent)?;
    let temp_file = temp.as_file();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp_file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let mut writer = temp_file;
    writer.write_all(data)?;
    writer.sync_all()?;

    #[cfg(unix)]
    {
        temp.persist(path).map(|_| ()).map_err(|e| e.error)
    }

    #[cfg(not(unix))]
    {
        // On Windows and other non-Unix systems, close the temp file handle
        // before renaming so it doesn't hold a sharing lock.
        let temp_path = temp.into_temp_path();
        atomic_rename(&temp_path, path)?;
        // Keep the temp file from being deleted on drop after successful rename.
        std::mem::forget(temp_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn rename_with_retry_succeeds_immediately() {
        let attempts = AtomicUsize::new(0);
        let sleeps = AtomicUsize::new(0);
        let res = rename_with_retry_impl(
            |_| {
                attempts.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            |_, _| {
                sleeps.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(res.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(sleeps.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn rename_with_retry_recovers_after_transient_failures() {
        let attempts = AtomicUsize::new(0);
        let sleeps = AtomicUsize::new(0);
        let mut slept_durations = Vec::new();
        let res = rename_with_retry_impl(
            |_| {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 3 {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "sharing violation",
                    ))
                } else {
                    Ok(())
                }
            },
            |_, dur| {
                sleeps.fetch_add(1, Ordering::SeqCst);
                slept_durations.push(dur);
            },
        );
        assert!(res.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        assert_eq!(sleeps.load(Ordering::SeqCst), 3);
        assert_eq!(slept_durations, vec![Duration::from_millis(10); 3]);
    }

    #[test]
    fn rename_with_retry_exhausts_ten_attempts() {
        let attempts = AtomicUsize::new(0);
        let sleeps = AtomicUsize::new(0);
        let res = rename_with_retry_impl(
            |_| {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "locked",
                ))
            },
            |_, _| {
                sleeps.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(res.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 10);
        assert_eq!(sleeps.load(Ordering::SeqCst), 9);
    }

    #[test]
    fn rename_with_retry_handles_windows_sharing_violation() {
        let attempts = AtomicUsize::new(0);
        let sleeps = AtomicUsize::new(0);
        let res = rename_with_retry_impl(
            |_| {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    Err(std::io::Error::from_raw_os_error(32))
                } else {
                    Ok(())
                }
            },
            |_, dur| {
                assert_eq!(dur, Duration::from_millis(10));
                sleeps.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(res.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(sleeps.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn atomic_write_creates_and_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        atomic_write(&file_path, b"initial content").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"initial content");

        atomic_write(&file_path, b"updated content").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"updated content");
    }

    #[test]
    fn atomic_write_handles_relative_path_without_parent() {
        let filename = format!("test_bare_{}.tmp", std::process::id());
        let bare_path = Path::new(&filename);
        let res = atomic_write(bare_path, b"bare content");
        let _ = fs::remove_file(bare_path);
        assert!(res.is_ok());
    }
}
