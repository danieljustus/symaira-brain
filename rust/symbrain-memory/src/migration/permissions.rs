//! Match the store owner's post-migration Unix file permissions.

use crate::StoreError;
use std::path::Path;

pub(crate) fn secure_files(path: &Path) -> Result<(), StoreError> {
    use std::{fs, os::unix::fs::PermissionsExt};

    // Go commits the migration before chmod. A chmod failure therefore reports
    // the failure while retaining the completed migration, without rollback.
    if fs::metadata(path).is_ok() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            StoreError::Io(format!("failed to set db file permissions: {error}"))
        })?;
    }
    for suffix in ["-wal", "-shm"] {
        let mut sibling = path.as_os_str().to_os_string();
        sibling.push(suffix);
        // Like Go, transient/missing WAL siblings do not fail a successful open.
        if fs::metadata(&sibling).is_ok() {
            let _ = fs::set_permissions(sibling, fs::Permissions::from_mode(0o600));
        }
    }
    Ok(())
}
