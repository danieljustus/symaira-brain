//! Windows-specific handle-relative directory capability and raw JSONL append implementation.

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, MetadataExt, OpenOptions};
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Validates a single path component for Windows filesystem safety.
///
/// Rejects control characters, invalid Windows characters, Alternate Data Stream
/// (ADS) stream colons, trailing dot/space aliases, and reserved DOS device names
/// (including superscript numerals 1, 2, 3).
///
/// # Errors
/// Returns [`io::ErrorKind::InvalidInput`] if the component is invalid or unsafe.
pub(crate) fn validate_windows_component(name: &OsStr) -> io::Result<()> {
    let raw = name.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: path component contains invalid Unicode",
        )
    })?;
    if raw.is_empty() || raw == "." || raw == ".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: path contains an unsafe component",
        ));
    }
    if raw.chars().any(|c| (c as u32) < 0x20) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: path component contains control characters",
        ));
    }
    if raw
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: path component contains invalid characters or ADS stream separator",
        ));
    }
    if raw.ends_with(' ') || raw.ends_with('.') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: path component has trailing dot or space alias",
        ));
    }
    if is_reserved_windows_device_name(raw) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("audit: path component uses reserved Windows device name: {raw}"),
        ));
    }
    Ok(())
}

/// Checks if a component stem matches reserved DOS device names.
///
/// Reserved names include CON, PRN, AUX, NUL, CONIN$, CONOUT$, COM1..9, LPT1..9,
/// and COM/LPT with superscripts 1, 2, 3.
#[must_use]
pub(crate) fn is_reserved_windows_device_name(name: &str) -> bool {
    let stem = match name.split_once('.') {
        Some((stem, _)) => stem,
        None => name,
    };
    let stem = stem.trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    if (upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.len() == 4
        && (b'1'..=b'9').contains(&upper.as_bytes()[3])
    {
        return true;
    }
    let upper_unicode = stem.to_uppercase();
    for prefix in ["COM", "LPT"] {
        if upper_unicode
            .strip_prefix(prefix)
            .is_some_and(|rest| matches!(rest, "¹" | "²" | "³"))
        {
            return true;
        }
    }
    false
}

/// Validates a Windows path for device namespaces, non-disk prefixes, drive-relative
/// traversal, `..` traversal, and unsafe component names.
///
/// # Errors
/// Returns [`io::ErrorKind::InvalidInput`] if the path or any component is unsafe.
pub(crate) fn validate_path(path: &Path) -> io::Result<()> {
    let raw = path.to_string_lossy();
    if raw.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        ));
    }
    if raw.starts_with(r"\\.\")
        || raw.starts_with(r"\\?\")
        || raw.starts_with(r"\??\")
        || raw.starts_with("//./")
        || raw.starts_with("//?/")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: device namespaces are not supported",
        ));
    }

    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        match component {
            Component::Prefix(prefix) => {
                if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "audit: device namespaces and non-disk prefixes are not supported",
                    ));
                }
                if !matches!(components.peek(), Some(Component::RootDir)) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "audit: drive-relative paths are not safe",
                    ));
                }
            }
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "audit: path contains an unsafe component",
                ));
            }
            Component::Normal(name) => {
                validate_windows_component(name)?;
            }
            Component::CurDir | Component::RootDir => {}
        }
    }
    Ok(())
}

/// Splits the target path into its parent path and target file name.
///
/// Pre-validates all components before returning the split parts.
///
/// # Errors
/// Returns an error if the path does not name a file, contains device namespaces,
/// contains `..` traversal, drive-relative prefixes, or has an unsafe component.
pub(crate) fn split_target(path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    validate_path(path)?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        )
    })?;
    validate_windows_component(name)?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Ok((parent.to_path_buf(), PathBuf::from(name)))
}

/// Creates (if requested) and opens a child directory relative to `parent` with
/// nofollow symlink and reparse point checks.
///
/// # Errors
/// Returns an error if directory creation fails, opening fails, or the opened handle
/// refers to a symlink or reparse point.
fn open_child_dir(parent: &Dir, name: &OsStr, create: bool) -> io::Result<Dir> {
    if create {
        match parent.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let next = parent.open_dir_nofollow(name)?;
    let meta = next.dir_metadata()?;
    if !meta.file_type().is_dir()
        || meta.file_type().is_symlink()
        || (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "audit: symlink or reparse point path component: {}",
                Path::new(name).display()
            ),
        ));
    }
    Ok(next)
}

/// Opens the parent directory component-by-component without following
/// reparse points or symlinks.
///
/// Pre-validates all components before creating any directories.
///
/// # Errors
/// Returns an error if any component is unsafe, a symlink/reparse point,
/// or cannot be created/opened.
pub(crate) fn open_parent(path: &Path, create: bool) -> io::Result<Dir> {
    validate_path(path)?;

    let mut components = path.components();
    let mut current = match components.next() {
        Some(Component::Prefix(prefix)) => {
            let _ = components.next();
            let mut anchor = prefix.as_os_str().to_os_string();
            anchor.push("\\");
            Dir::open_ambient_dir(PathBuf::from(anchor), cap_std::ambient_authority())?
        }
        Some(Component::RootDir) => {
            Dir::open_ambient_dir(Path::new("\\"), cap_std::ambient_authority())?
        }
        Some(Component::CurDir) => {
            Dir::open_ambient_dir(Path::new("."), cap_std::ambient_authority())?
        }
        Some(Component::Normal(name)) => {
            let root = Dir::open_ambient_dir(Path::new("."), cap_std::ambient_authority())?;
            open_child_dir(&root, name, create)?
        }
        Some(Component::ParentDir) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit: path contains an unsafe component",
            ));
        }
        None => Dir::open_ambient_dir(Path::new("."), cap_std::ambient_authority())?,
    };
    for component in components {
        match component {
            Component::Normal(name) => {
                current = open_child_dir(&current, name, create)?;
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "audit: path contains an unsafe component",
                ));
            }
        }
    }
    Ok(current)
}

/// Appends a raw single-line record to the target file in the parent directory.
///
/// # Errors
/// Returns an error if the target is not a regular file, is a symlink or reparse point,
/// or if a short write occurs.
pub(crate) fn append_to_parent(parent: &Dir, name: &Path, record: &[u8]) -> io::Result<()> {
    let mut data = Vec::with_capacity(record.len() + 1);
    data.extend_from_slice(record);
    data.push(b'\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    options.follow(FollowSymlinks::No);
    let mut file = parent.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || (metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target must be a regular file and not a reparse point",
        ));
    }
    let written = file.write(&data)?;
    if written != data.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("audit: short append write: {written}/{} bytes", data.len()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reserved_windows_device_names_match_exact_list() {
        assert!(is_reserved_windows_device_name("CON"));
        assert!(is_reserved_windows_device_name("PRN"));
        assert!(is_reserved_windows_device_name("AUX"));
        assert!(is_reserved_windows_device_name("NUL"));
        assert!(is_reserved_windows_device_name("CONIN$"));
        assert!(is_reserved_windows_device_name("CONOUT$"));
        assert!(is_reserved_windows_device_name("COM1"));
        assert!(is_reserved_windows_device_name("COM9"));
        assert!(is_reserved_windows_device_name("LPT1"));
        assert!(is_reserved_windows_device_name("LPT9"));
        assert!(is_reserved_windows_device_name("COM¹"));
        assert!(is_reserved_windows_device_name("COM²"));
        assert!(is_reserved_windows_device_name("COM³"));
        assert!(is_reserved_windows_device_name("LPT¹"));
        assert!(is_reserved_windows_device_name("LPT²"));
        assert!(is_reserved_windows_device_name("LPT³"));

        // Non-reserved names (COM0, LPT0, COM4-superscript, LPT4-superscript, COM10, normal names)
        assert!(!is_reserved_windows_device_name("COM0"));
        assert!(!is_reserved_windows_device_name("LPT0"));
        assert!(!is_reserved_windows_device_name("COM⁴"));
        assert!(!is_reserved_windows_device_name("LPT⁴"));
        assert!(!is_reserved_windows_device_name("COM10"));
        assert!(!is_reserved_windows_device_name("audit"));
        assert!(!is_reserved_windows_device_name("normal_log"));
    }

    #[test]
    fn open_parent_protects_against_rename_and_appends_exact_bytes() {
        let temp = tempdir().expect("tempdir");
        let parent_dir = temp.path().join("parent_log_dir");
        let parent = open_parent(&parent_dir, true).expect("open parent");

        // Attempt rename of opened parent to prove protection while handle is held
        let renamed = temp.path().join("renamed_parent_dir");
        let rename_result = std::fs::rename(&parent_dir, &renamed);
        assert!(
            rename_result.is_err(),
            "opened parent directory handle must prevent renaming on Windows"
        );

        // Append to parent and assert exact bytes
        let file_name = Path::new("audit.log");
        let record = br#"{"test":"exact_payload_windows"}"#;
        append_to_parent(&parent, file_name, record).expect("append to parent");

        let log_file = parent_dir.join(file_name);
        let content = std::fs::read(&log_file).expect("read audit log");
        let mut expected = record.to_vec();
        expected.push(b'\n');
        assert_eq!(content, expected);
    }

    #[test]
    fn split_target_normalizes_bare_target_to_current_directory() {
        let (parent, name) = split_target(Path::new("audit.log")).expect("split bare target");
        assert_eq!(parent, Path::new("."));
        assert_eq!(name, Path::new("audit.log"));

        let (parent_nested, name_nested) =
            split_target(Path::new("logs\\audit.log")).expect("split nested target");
        assert_eq!(parent_nested, Path::new("logs"));
        assert_eq!(name_nested, Path::new("audit.log"));

        assert!(split_target(Path::new("")).is_err());
    }
}
