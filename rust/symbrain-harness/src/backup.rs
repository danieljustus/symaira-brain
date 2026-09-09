use chrono::{DateTime, Utc};
use std::io;
use std::path::{Path, PathBuf};

use crate::write::AtomicFile;

/// Creates a timestamped backup, returning `None` when the source is absent.
///
/// # Errors
/// Returns an I/O error when the source cannot be read or the atomic backup cannot be written.
pub fn backup(path: &Path) -> Result<Option<PathBuf>, crate::HarnessError> {
    backup_at(path, Utc::now())
}

/// Backup variant with an injected clock for deterministic contract tests.
///
/// # Errors
/// Returns an I/O error when the source cannot be read or the atomic backup cannot be written.
pub fn backup_at(
    path: &Path,
    timestamp: DateTime<Utc>,
) -> Result<Option<PathBuf>, crate::HarnessError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        crate::HarnessError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "backup target has no file name",
        ))
    })?;
    let capability = AtomicFile::open(parent, Path::new(name), path.to_path_buf(), false)?;
    let snapshot = match capability.read_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(crate::HarnessError::Io(error)),
    };
    let target =
        capability.backup_snapshot(&snapshot, &timestamp.format("%Y%m%dT%H%M%SZ").to_string())?;
    Ok(Some(target))
}
