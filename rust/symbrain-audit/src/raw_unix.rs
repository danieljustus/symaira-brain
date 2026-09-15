//! Unix-specific handle-relative directory capability and raw JSONL append implementation.

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// Appends a raw single-line record to the target file in the parent directory.
///
/// # Errors
/// Returns an error if the target is not a regular file or a short write occurs.
pub(crate) fn append_to_parent(parent: &Dir, name: &Path, record: &[u8]) -> io::Result<()> {
    let mut data = Vec::with_capacity(record.len() + 1);
    data.extend_from_slice(record);
    data.push(b'\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    options.follow(FollowSymlinks::No);
    options.mode(0o600);
    options.nonblock(true);
    let mut file = parent.open_with(name, &options)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target must be a regular file",
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

/// Splits the target path into its parent path and target file name.
///
/// # Errors
/// Returns an error if the path does not name a file.
pub(crate) fn split_target(path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        )
    })?;
    if matches!(name.to_str(), Some("." | "..")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok((parent.to_path_buf(), PathBuf::from(name)))
}

/// Opens the parent directory component-by-component without following symlinks.
///
/// # Errors
/// Returns an error if any component is unsafe, a symlink, or cannot be created/opened.
#[allow(clippy::collapsible_if)]
pub(crate) fn open_parent(path: &Path, create: bool) -> io::Result<Dir> {
    let path = physical_path(path);
    let mut current = if path.is_absolute() {
        Dir::open_ambient_dir(Path::new("/"), cap_std::ambient_authority())?
    } else {
        Dir::open_ambient_dir(Path::new("."), cap_std::ambient_authority())?
    };
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "audit: path contains an unsafe component",
                ));
            }
        };
        if create {
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            match current.create_dir_with(name, &builder) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        if let Ok(metadata) = current.symlink_metadata(name) {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("audit: symlink path component: {}", name.display()),
                ));
            }
        }
        let next = current.open_dir_nofollow(name)?;
        current = next;
    }
    Ok(current)
}

#[cfg(target_os = "macos")]
fn physical_path(path: &Path) -> PathBuf {
    if path == Path::new("/var") || path.starts_with("/var/") {
        PathBuf::from("/private").join(path.strip_prefix("/").unwrap_or(path))
    } else {
        path.to_path_buf()
    }
}

#[cfg(not(target_os = "macos"))]
fn physical_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}
