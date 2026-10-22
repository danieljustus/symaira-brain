//! Database directory creation with the shipped store's exact semantics.
//!
//! The Go store creates the database's directory with a symlink-refusing walk
//! (mode `0700`, process umask applied), not with `MkdirAll`. Directories keep
//! the narrower mode, non-root-owned symlinks and non-directories are refused,
//! and every failure carries the Go `PathError` text.

use std::path::Path;

/// Go `filepath.Dir`: everything up to the last separator, cleaned.
#[cfg(unix)]
pub(crate) fn go_dir(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    let end = bytes
        .iter()
        .rposition(|&byte| byte == b'/')
        .map_or(0, |index| index + 1);
    go_clean(&bytes[..end])
}

/// Go `filepath.Clean` for slash-separated byte paths.
#[cfg(unix)]
pub(crate) fn go_clean(path: &[u8]) -> Vec<u8> {
    let rooted = path.first() == Some(&b'/');
    let mut stack: Vec<&[u8]> = Vec::new();
    for element in path.split(|&byte| byte == b'/') {
        match element {
            b"" | b"." => {}
            b".." => {
                if stack.last().is_some_and(|last| *last != b"..") {
                    stack.pop();
                } else if !rooted {
                    stack.push(element);
                }
            }
            _ => stack.push(element),
        }
    }
    let mut cleaned = Vec::new();
    if rooted {
        cleaned.push(b'/');
    }
    cleaned.extend(stack.join(&b'/'));
    if cleaned.is_empty() {
        cleaned.push(b'.');
    }
    cleaned
}

/// Go `errno` text: the lower-cased platform message without Rust's suffix.
fn errno_text(error: &std::io::Error) -> String {
    let text = error.to_string();
    let text = text
        .rsplit_once(" (os error ")
        .map_or(text.as_str(), |(message, _)| message);
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(characters).collect()
    })
}

#[cfg(unix)]
const ENOTDIR: i32 = 20;
#[cfg(all(unix, target_os = "linux"))]
const ELOOP: i32 = 40;
#[cfg(all(unix, not(target_os = "linux")))]
const ELOOP: i32 = 62;

#[cfg(unix)]
fn path_error(operation: &str, path: &[u8], error: &std::io::Error) -> String {
    format!(
        "{operation} {}: {}",
        String::from_utf8_lossy(path),
        errno_text(error)
    )
}

/// Creates `directory` (relative paths against the cwd) like `SafeMkdirAll`.
///
/// # Errors
/// Returns the shipped `PathError` text without any outer wrapping.
#[cfg(unix)]
pub(crate) fn safe_mkdir_all(directory: &[u8]) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    let absolute = if directory.first() == Some(&b'/') {
        go_clean(directory)
    } else {
        let mut joined = std::env::current_dir()
            .map(|cwd| cwd.into_os_string().into_vec())
            .unwrap_or_default();
        joined.push(b'/');
        joined.extend_from_slice(directory);
        go_clean(&joined)
    };
    let mut current: Vec<u8> = vec![b'/'];
    let components = absolute
        .split(|&byte| byte == b'/')
        .filter(|part| !part.is_empty());
    for component in components {
        if current.last() != Some(&b'/') {
            current.push(b'/');
        }
        current.extend_from_slice(component);
        let target = Path::new(OsStr::from_bytes(&current)).to_path_buf();
        let metadata = match std::fs::symlink_metadata(&target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&target)
                    .map_err(|error| path_error("mkdir", &current, &error))?;
                continue;
            }
            Err(error) => return Err(path_error("lstat", &current, &error)),
        };
        if metadata.file_type().is_symlink() {
            // Root-owned symlinks (such as macOS /var) are trusted system
            // links; every other symlink is refused.
            if metadata.uid() != 0 {
                let error = std::io::Error::from_raw_os_error(ELOOP);
                return Err(path_error("mkdir", &current, &error));
            }
            let real = std::fs::canonicalize(&target)
                .map_err(|error| path_error("lstat", &current, &error))?;
            current = real.into_os_string().into_vec();
            continue;
        }
        if !metadata.is_dir() {
            let error = std::io::Error::from_raw_os_error(ENOTDIR);
            return Err(path_error("mkdir", &current, &error));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn go_dir(path: &Path) -> Vec<u8> {
    path.parent()
        .map_or_else(
            || std::path::PathBuf::from("."),
            |parent| {
                if parent.as_os_str().is_empty() {
                    std::path::PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                }
            },
        )
        .to_string_lossy()
        .into_owned()
        .into_bytes()
}

#[cfg(not(unix))]
pub(crate) fn safe_mkdir_all(directory: &[u8]) -> Result<(), String> {
    let directory = String::from_utf8_lossy(directory).into_owned();
    let mut current = std::path::PathBuf::new();
    let absolute = std::path::absolute(&directory).map_err(|error| error.to_string())?;
    for component in absolute.components() {
        current.push(component);
        match std::fs::metadata(&current) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "mkdir {}: path is not a regular file",
                    current.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    format!("mkdir {}: {}", current.display(), errno_text(&error))
                })?;
            }
            Err(error) => {
                return Err(format!("{}: {}", current.display(), errno_text(&error)));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn clean_and_dir_follow_go_semantics() {
        for (input, expected) in [
            ("", "."),
            ("x.db", "."),
            ("a/b", "a"),
            ("a/b/", "a/b"),
            ("/x", "/"),
            ("/a//b/../c/d.db", "/a/c"),
            ("../x", ".."),
            ("a/../../x", ".."),
            ("/../x", "/"),
        ] {
            let got = go_dir(Path::new(input));
            assert_eq!(String::from_utf8(got).unwrap(), expected, "{input}");
        }
        assert_eq!(go_clean(b"/a/./b/../../.."), b"/");
        assert_eq!(go_clean(b"a/.."), b".");
    }

    #[test]
    fn errno_text_matches_the_go_error_table() {
        let error = std::io::Error::from_raw_os_error(ENOTDIR);
        assert_eq!(errno_text(&error), "not a directory");
        let error = std::io::Error::from_raw_os_error(ELOOP);
        assert_eq!(errno_text(&error), "too many levels of symbolic links");
        let error = std::io::Error::from_raw_os_error(13);
        assert_eq!(errno_text(&error), "permission denied");
    }
}
