//! Per-app backup slot and cleanup of copies left behind by dead runs.
//!
//! The slot is an advisory `flock`: the kernel drops it when the holder dies
//! (even on SIGKILL), so "can I take the lock?" is exact liveness evidence
//! without pid files or `/proc` probing.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Exclusive right to back up one app; released on drop or process death.
pub(crate) struct Slot {
    _file: File,
}

/// Takes the slot without waiting; `None` means a live run already holds it.
pub(crate) fn try_acquire(store: &Path, app: &str) -> io::Result<Option<Slot>> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(store.join(format!(".{app}.lock")))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Slot { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Removes `.partial` trees and temp files of runs that died mid-copy.
///
/// Callers must hold the [`Slot`]: only then is every such leftover an orphan.
pub(crate) fn remove_orphans(store: &Path, app: &str) {
    let Ok(entries) = fs::read_dir(store) else {
        return;
    };
    let partial = format!(".{app}-");
    let temp = format!("{app}.tmp-");
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // A timestamp (`20261106T...`) follows the app name, so `symbrain`
        // never claims the partials of an app called `symbrain-extra`.
        let stamped = name
            .strip_prefix(&partial)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        if stamped && name.ends_with(".partial") {
            let _ = fs::remove_dir_all(entry.path());
        } else if name.starts_with(&temp) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// True while a live run holds the backup slot. Never creates the lock file.
pub(crate) fn is_held(store: &Path, app: &str) -> bool {
    let Ok(file) = File::open(store.join(format!(".{app}.lock"))) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}
