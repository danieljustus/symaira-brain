//! Go-parity configuration reads for `harness list`.
//!
//! Mirrors `readConfigFile` and `openConfigFile` in `internal/harness`: every
//! directory component is opened without following symlinks and the file is
//! opened `O_NOFOLLOW`. Failures carry Go's exact error text (`os.PathError`
//! wording and the lower-cased `syscall.Errno` strings).

use std::io;
use std::path::Path;

use crate::MAX_CONFIG_BYTES;

/// Why a configuration could not be read.
pub(crate) enum ReadFailure {
    /// Go's `errors.Is(err, os.ErrNotExist)`: not an inventory error.
    NotFound,
    /// The error text Go stores in `ConfigInventory.Error`.
    Message(String),
}

/// Go's `syscall.Errno.Error()` for an operating-system error.
pub(crate) fn go_io_text(error: &io::Error) -> String {
    let text = error.to_string();
    let text = match (error.raw_os_error(), text.rfind(" (os error ")) {
        (Some(_), Some(index)) => text[..index].to_owned(),
        _ => text,
    };
    if cfg!(windows) {
        return text;
    }
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

/// Reads `path` like Go's `readConfigFile`. `capability` is only used where
/// the platform has no Go-compatible component walk (non-Unix targets).
#[cfg(unix)]
pub(crate) fn read_config(
    path: &Path,
    _capability: Option<(std::path::PathBuf, std::path::PathBuf)>,
) -> Result<Vec<u8>, ReadFailure> {
    use std::io::Read;

    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
    use cap_std::ambient_authority;
    use cap_std::fs::{Dir, OpenOptions};

    use crate::go_quote::quote_str;
    use crate::inventory_read::clean_path;

    let display = path.to_string_lossy().into_owned();
    let open_failure = |error: &io::Error| {
        if error.kind() == io::ErrorKind::NotFound {
            ReadFailure::NotFound
        } else {
            ReadFailure::Message(format!("open {display}: {}", go_io_text(error)))
        }
    };

    let directory = clean_path(path.parent().unwrap_or_else(|| Path::new(".")));
    let directory = if cfg!(target_os = "macos") {
        normalize_system_alias(&directory)
    } else {
        directory
    };
    let name = path
        .file_name()
        .ok_or_else(|| ReadFailure::Message(format!("open {display}: invalid argument")))?;
    let anchor = if directory.is_absolute() { "/" } else { "." };
    let mut current =
        Dir::open_ambient_dir(anchor, ambient_authority()).map_err(|error| open_failure(&error))?;
    for component in directory.to_string_lossy().split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(ReadFailure::Message(
                "configuration directory contains parent component".to_owned(),
            ));
        }
        current = match current.open_dir_nofollow(component) {
            Ok(next) => next,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ReadFailure::NotFound);
            }
            Err(error) => {
                return Err(ReadFailure::Message(format!(
                    "open configuration directory component {}: {}",
                    quote_str(component),
                    go_io_text(&error)
                )));
            }
        };
    }

    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let file = current
        .open_with(name, &options)
        .map_err(|error| open_failure(&error))?;
    let metadata = file.metadata().map_err(|error| {
        ReadFailure::Message(format!(
            "stat configuration: stat {display}: {}",
            go_io_text(&error)
        ))
    })?;
    if !metadata.is_file() {
        return Err(ReadFailure::Message(
            "configuration target is not a regular file".to_owned(),
        ));
    }
    let too_large = || {
        ReadFailure::Message(format!(
            "configuration target exceeds maximum size of {MAX_CONFIG_BYTES} bytes"
        ))
    };
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ReadFailure::Message(format!(
                "read configuration: read {display}: {}",
                go_io_text(&error)
            ))
        })?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Go's `normalizeSystemAlias` (macOS only).
#[cfg(unix)]
fn normalize_system_alias(path: &Path) -> std::path::PathBuf {
    for (alias, real) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
        if let Ok(rest) = path.strip_prefix(alias) {
            return Path::new(real).join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(not(unix))]
pub(crate) fn read_config(
    path: &Path,
    capability: Option<(std::path::PathBuf, std::path::PathBuf)>,
) -> Result<Vec<u8>, ReadFailure> {
    let display = path.to_string_lossy().into_owned();
    let Some((trusted_root, relative_path)) = capability else {
        return Err(ReadFailure::Message("resolve config capability".to_owned()));
    };
    let failure = |error: &io::Error| {
        if error.kind() == io::ErrorKind::NotFound {
            ReadFailure::NotFound
        } else if error.to_string() == "configuration target is not a regular file" {
            ReadFailure::Message(error.to_string())
        } else if error.to_string() == "configuration target exceeds maximum size" {
            ReadFailure::Message(format!(
                "configuration target exceeds maximum size of {MAX_CONFIG_BYTES} bytes"
            ))
        } else {
            ReadFailure::Message(format!("open {display}: {}", go_io_text(error)))
        }
    };
    let file = crate::AtomicFile::open(&trusted_root, &relative_path, path.to_path_buf(), false)
        .map_err(|error| failure(&error))?;
    file.read_snapshot()
        .map(|snapshot| snapshot.bytes)
        .map_err(|error| failure(&error))
}
