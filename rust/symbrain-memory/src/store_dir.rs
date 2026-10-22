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

/// The kinds of `std::path::Component`, constructible on every platform so the
/// Windows walk plan can be unit-tested everywhere.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PartKind {
    /// A Windows prefix such as `C:`, `\\?\C:` or `\\server\share`.
    Prefix,
    RootDir,
    CurDir,
    ParentDir,
    Normal,
}

/// One step of the Windows directory walk.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Part of the volume root (`vol + separator` in Go): never inspected.
    Root,
    /// A directory name that is verified or created.
    Check,
    /// `..` after the lexical clean has nothing left to remove: dropped.
    Skip,
    /// `..` removes the previous checked directory name.
    Pop,
    /// `.`: nothing to do.
    Ignore,
}

/// Plans the component-by-component walk of `SafeMkdirAll` on Windows.
///
/// Go starts at `filepath.VolumeName(abs) + separator` and only looks at the
/// names below it. Inspecting the prefix itself is wrong: `\\?\C:` (the shape
/// `Path::canonicalize` returns) names the volume device, not its root
/// directory, so a metadata call on it is not a directory and the walk would
/// reject every verbatim path (#769).
#[cfg(any(windows, test))]
fn walk_plan(parts: &[PartKind]) -> Vec<Step> {
    let mut plan = Vec::with_capacity(parts.len());
    let mut depth = 0_usize;
    let mut in_root = true;
    for part in parts {
        let step = match part {
            PartKind::Prefix | PartKind::RootDir if in_root => Step::Root,
            // A prefix or root in the middle cannot occur in an absolute
            // path; treat it as a name so it is verified, not trusted.
            PartKind::Prefix | PartKind::RootDir | PartKind::Normal => Step::Check,
            PartKind::CurDir => Step::Ignore,
            PartKind::ParentDir if depth > 0 => Step::Pop,
            PartKind::ParentDir => Step::Skip,
        };
        match step {
            Step::Check => {
                in_root = false;
                depth += 1;
            }
            Step::Pop => depth -= 1,
            Step::Root | Step::Skip | Step::Ignore => {}
        }
        plan.push(step);
    }
    plan
}

#[cfg(not(unix))]
pub(crate) fn safe_mkdir_all(directory: &[u8]) -> Result<(), String> {
    use std::path::Component;

    let directory = String::from_utf8_lossy(directory).into_owned();
    let absolute = std::path::absolute(&directory).map_err(|error| error.to_string())?;
    let components: Vec<Component<'_>> = absolute.components().collect();
    let kinds: Vec<PartKind> = components
        .iter()
        .map(|component| match component {
            Component::Prefix(_) => PartKind::Prefix,
            Component::RootDir => PartKind::RootDir,
            Component::CurDir => PartKind::CurDir,
            Component::ParentDir => PartKind::ParentDir,
            Component::Normal(_) => PartKind::Normal,
        })
        .collect();
    let mut current = std::path::PathBuf::new();
    for (component, step) in components.into_iter().zip(walk_plan(&kinds)) {
        match step {
            Step::Root => {
                current.push(component);
                continue;
            }
            Step::Ignore | Step::Skip => continue,
            Step::Pop => {
                current.pop();
                continue;
            }
            Step::Check => current.push(component),
        }
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

#[cfg(test)]
mod walk_plan_tests {
    use super::PartKind::{CurDir, Normal, ParentDir, Prefix, RootDir};
    use super::{Step, walk_plan};

    #[test]
    fn the_volume_root_is_never_inspected() {
        // `\\?\D:\a\data` canonicalized on Windows: Prefix, RootDir, names.
        assert_eq!(
            walk_plan(&[Prefix, RootDir, Normal, Normal]),
            [Step::Root, Step::Root, Step::Check, Step::Check]
        );
        // A bare root and a drive-relative prefix without a root.
        assert_eq!(walk_plan(&[RootDir, Normal]), [Step::Root, Step::Check]);
        assert_eq!(walk_plan(&[Prefix, Normal]), [Step::Root, Step::Check]);
        assert_eq!(walk_plan(&[Normal, Normal]), [Step::Check, Step::Check]);
        assert!(walk_plan(&[]).is_empty());
    }

    #[test]
    fn dot_segments_follow_the_lexical_clean() {
        // Verbatim paths are not normalized by `std::path::absolute`.
        assert_eq!(
            walk_plan(&[Prefix, RootDir, Normal, CurDir, Normal, ParentDir, Normal]),
            [
                Step::Root,
                Step::Root,
                Step::Check,
                Step::Ignore,
                Step::Check,
                Step::Pop,
                Step::Check
            ]
        );
        // `..` at the root has nothing to remove (Go clamps it).
        assert_eq!(
            walk_plan(&[Prefix, RootDir, ParentDir, Normal]),
            [Step::Root, Step::Root, Step::Skip, Step::Check]
        );
    }

    #[test]
    fn a_misplaced_root_is_verified_not_trusted() {
        assert_eq!(
            walk_plan(&[Prefix, RootDir, Normal, RootDir]),
            [Step::Root, Step::Root, Step::Check, Step::Check]
        );
    }
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

    #[cfg(target_os = "macos")]
    #[test]
    fn root_owned_symlink_is_trusted_but_user_owned_is_refused() {
        use std::os::unix::fs::MetadataExt;

        // macOS `/tmp` -> `private/tmp` is a root-owned system link.
        assert_eq!(std::fs::symlink_metadata("/tmp").unwrap().uid(), 0);
        let unique = format!("symbrain-store-dir-{}", std::process::id());
        let through = format!("/tmp/{unique}/nested");
        let real = std::path::PathBuf::from(format!("/private/tmp/{unique}"));
        let result = safe_mkdir_all(through.as_bytes());
        let created = real.join("nested").is_dir();
        let _ = std::fs::remove_dir_all(&real);
        assert_eq!(result, Ok(()));
        assert!(
            created,
            "directory created beneath the resolved link target"
        );

        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let refused = root.join("link/sub");
        let result = safe_mkdir_all(refused.as_os_str().as_encoded_bytes());
        let _ = std::fs::remove_dir_all(&root);
        let message = result.unwrap_err();
        assert!(
            message.ends_with("too many levels of symbolic links"),
            "{message}"
        );
    }
}
