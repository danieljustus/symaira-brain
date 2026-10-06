//! Go `MkdirAll`'s actual failing path, preserved through managed diagnostics.
use std::path::{Path, PathBuf};

use crate::{GoText, ManagedError};

pub(crate) fn create_bin_dir(path: &Path) -> Result<(), ManagedError> {
    mkdir_all(path).map_err(|(failed, error)| {
        let mut message = GoText::path("managed: mkdir ", path, ": mkdir ");
        message.push(&symbrain_core::config::os_bytes(failed.as_os_str()));
        ManagedError::RawContext(
            message.with_suffix(format!(": {}", crate::format_io_error(&error)).as_bytes()),
        )
    })
}

fn mkdir_all(path: &Path) -> Result<(), (PathBuf, std::io::Error)> {
    if let Ok(metadata) = std::fs::metadata(path) {
        return if metadata.is_dir() {
            Ok(())
        } else {
            // Go's Stat fast path intentionally returns syscall.ENOTDIR.
            // The pinned Windows SDK maps ENOTDIR to ERROR_PATH_NOT_FOUND.
            #[cfg(unix)]
            let code = 20;
            #[cfg(windows)]
            let code = 3;
            Err((path.to_owned(), std::io::Error::from_raw_os_error(code)))
        };
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        mkdir_all(parent)?;
    }
    #[cfg(unix)]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o755);
    }
    builder
        .create(path)
        .map_err(|error| (path.to_owned(), error))
        .or_else(|error| {
            // Go rechecks Lstat after Mkdir to tolerate a directory created by
            // another process and lexical suffixes such as foo/.
            if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
                Ok(())
            } else {
                Err(error)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::create_bin_dir;

    #[test]
    fn obstruction_reports_the_actual_ancestor_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join(".symaira");
        std::fs::write(&blocked, b"owned obstruction").unwrap();
        let target = blocked.join("bin");
        let error = create_bin_dir(&target).unwrap_err().into_go_text();
        #[cfg(unix)]
        let code = 20;
        #[cfg(windows)]
        let code = 3;
        let detail = crate::format_io_error(&std::io::Error::from_raw_os_error(code));
        assert_eq!(
            error.as_ref(),
            format!(
                "managed: mkdir {}: mkdir {}: {detail}",
                target.display(),
                blocked.display()
            )
            .as_bytes()
        );
        assert_eq!(std::fs::read(blocked).unwrap(), b"owned obstruction");
    }
}
