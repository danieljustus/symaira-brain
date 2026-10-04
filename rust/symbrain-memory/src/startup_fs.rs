//! Filesystem ordering and permissions of the frozen Memory startup owner.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use symbrain_core::{GoText, go_path};

use crate::{Store, StoreError};

pub(super) fn path_error(operation: &str, path: &Path, error: &io::Error) -> GoText {
    let message = match error.raw_os_error() {
        #[cfg(unix)]
        Some(2) => "no such file or directory".to_owned(),
        #[cfg(unix)]
        Some(13) => "permission denied".to_owned(),
        #[cfg(unix)]
        Some(20) => "not a directory".to_owned(),
        #[cfg(unix)]
        Some(21) => "is a directory".to_owned(),
        #[cfg(target_os = "linux")]
        Some(40) => "too many levels of symbolic links".to_owned(),
        #[cfg(all(unix, not(target_os = "linux")))]
        Some(62) => "too many levels of symbolic links".to_owned(),
        #[cfg(windows)]
        Some(2) => "The system cannot find the file specified.".to_owned(),
        #[cfg(windows)]
        Some(3) => "The system cannot find the path specified.".to_owned(),
        #[cfg(windows)]
        Some(5) => "Access is denied.".to_owned(),
        _ => {
            let text = error.to_string();
            let message = error
                .raw_os_error()
                .map(|code| format!(" (os error {code})"));
            let text = message
                .as_ref()
                .and_then(|suffix| text.strip_suffix(suffix))
                .unwrap_or(&text);
            if cfg!(unix) {
                text.to_lowercase()
            } else {
                text.to_owned()
            }
        }
    };
    let mut bytes = format!("{operation} ").into_bytes();
    bytes.extend(go_path::os_bytes(path.as_os_str()));
    bytes.extend(format!(": {message}").as_bytes());
    bytes.into()
}

pub(super) struct ReadError {
    pub error: io::Error,
    pub text: GoText,
}

pub(super) fn read_file(path: &Path) -> Result<Vec<u8>, ReadError> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|error| ReadError {
        text: path_error("open", path, &error),
        error,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| ReadError {
        text: path_error("read", path, &error),
        error,
    })?;
    Ok(bytes)
}

pub(super) fn mkdir_private(path: &Path) -> Result<(), GoText> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| path_error("mkdir", path, &error))
}

pub(super) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), GoText> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| path_error("open", path, &error))?;
    file.write_all(bytes)
        .map_err(|error| path_error("write", path, &error))
}

fn safe_database_directory(path: &Path) -> Result<(), GoText> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| GoText::from(error.to_string()))?
            .join(path)
    };
    let cleaned = go_path::clean_bytes(&go_path::os_bytes(absolute.as_os_str()), cfg!(windows));
    let absolute = PathBuf::from(go_path::from_bytes(&cleaned));
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component);
        #[cfg(windows)]
        let metadata = fs::metadata(&current);
        #[cfg(not(windows))]
        let metadata = fs::symlink_metadata(&current);
        match metadata {
            Ok(metadata) => {
                #[cfg(unix)]
                if metadata.file_type().is_symlink() {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.uid() != 0 {
                        return Err(path_error(
                            "mkdir",
                            &current,
                            &io::Error::from_raw_os_error(if cfg!(target_os = "linux") {
                                40
                            } else {
                                62
                            }),
                        ));
                    }
                    current = fs::canonicalize(&current)
                        .map_err(|error| path_error("lstat", &current, &error))?;
                    continue;
                }
                if !metadata.is_dir() {
                    #[cfg(unix)]
                    let error = io::Error::from_raw_os_error(20);
                    #[cfg(not(unix))]
                    let error = io::Error::new(io::ErrorKind::NotADirectory, "unsafe path");
                    return Err(path_error("mkdir", &current, &error));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // One component at a time: existing permissions remain intact.
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder
                    .create(&current)
                    .map_err(|error| path_error("mkdir", &current, &error))?;
            }
            Err(error) => return Err(path_error("lstat", &current, &error)),
        }
    }
    Ok(())
}

pub(super) fn open_database(path: &Path) -> Result<Store, GoText> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    safe_database_directory(parent).map_err(|error| {
        error.with_prefix("failed to open sqlite database: failed to create database directory: ")
    })?;
    let store = Store::open(path).map_err(|error| {
        let detail = match &error {
            StoreError::Sql(rusqlite::Error::SqliteFailure(code, _))
                if code.extended_code == 14 =>
            {
                "unable to open database file: out of memory (14)".to_owned()
            }
            _ => error.to_string(),
        };
        GoText::from(format!(
            "failed to open sqlite database: failed to open sqlite database: {detail}"
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path).is_ok() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
                path_error("chmod", path, &error).with_prefix("failed to set db file permissions: ")
            })?;
        }
        for suffix in [b"-wal".as_slice(), b"-shm"] {
            let mut bytes = go_path::os_bytes(path.as_os_str());
            bytes.extend(suffix);
            let sibling = PathBuf::from(go_path::from_bytes(&bytes));
            if fs::metadata(&sibling).is_ok() {
                let _ = fs::set_permissions(sibling, fs::Permissions::from_mode(0o600));
            }
        }
    }
    // Go's CLI process leaves its open Memory connection to process exit.
    // Preserve committed WAL state rather than introducing a native-only
    // checkpoint/removal when the gateway owner drops at EOF.
    store
        .lock()
        .and_then(|connection| {
            connection
                .set_db_config(
                    rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
                    true,
                )
                .map_err(Into::into)
        })
        .map_err(|error| GoText::from(error.to_string()))?;
    Ok(store)
}
