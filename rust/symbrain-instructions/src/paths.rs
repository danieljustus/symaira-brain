use ambient_authority::ambient_authority;
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
#[cfg(unix)]
use cap_std::fs::{Permissions, PermissionsExt};
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn open_source_parent(parent: &Path) -> io::Result<Dir> {
    open_path_no_follow(parent, false)
}

/// Resolves macOS's fixed `/var` compatibility alias without permitting any
/// caller-controlled symlink component.
#[cfg(all(unix, target_os = "macos"))]
fn physical_path(path: &Path) -> PathBuf {
    if path == Path::new("/var") || path.starts_with("/var/") {
        PathBuf::from("/private").join(path.strip_prefix("/").unwrap_or(path))
    } else {
        path.to_path_buf()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn physical_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Opens a path component-by-component without following symlinks/reparse
/// points. Missing components are created only when requested.
#[cfg(unix)]
fn open_path_no_follow(path: &Path, create: bool) -> io::Result<Dir> {
    let path = physical_path(path);
    let mut current = if path.is_absolute() {
        Dir::open_ambient_dir(Path::new("/"), ambient_authority())?
    } else {
        Dir::open_ambient_dir(Path::new("."), ambient_authority())?
    };
    for component in path.components() {
        let name = match component {
            std::path::Component::RootDir | std::path::Component::CurDir => continue,
            std::path::Component::Normal(name) => name,
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path contains an unsafe component",
                ));
            }
        };
        let mut created = false;
        if create {
            match current.create_dir(name) {
                Ok(()) => created = true,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let next = current.open_dir_nofollow(name)?;
        if created {
            next.set_permissions(".", Permissions::from_mode(0o700))?;
        }
        current = next;
    }
    Ok(current)
}

/// Opens a Windows path component-by-component from a filesystem anchor.
///
/// The only ambient opens here are the drive/UNC root or the already-selected
/// process working directory. Every caller-controlled component is then opened
/// relative to the retained directory capability with reparse-point following
/// disabled. In particular, this must not be replaced by opening `path.parent()`
/// ambiently: an intermediate junction could otherwise redirect the walk.
#[cfg(not(unix))]
fn open_path_no_follow(path: &Path, create: bool) -> io::Result<Dir> {
    let mut components = path.components();
    let mut pending = Vec::new();
    let current = match components.next() {
        Some(std::path::Component::Prefix(prefix)) => {
            if !matches!(components.next(), Some(std::path::Component::RootDir)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "drive-relative paths are not safe",
                ));
            }
            let anchor = PathBuf::from(format!("{}\\", prefix.as_os_str().to_string_lossy()));
            Dir::open_ambient_dir(&anchor, ambient_authority())?
        }
        Some(std::path::Component::RootDir) => {
            Dir::open_ambient_dir(Path::new("\\"), ambient_authority())?
        }
        Some(std::path::Component::CurDir) => {
            Dir::open_ambient_dir(Path::new("."), ambient_authority())?
        }
        Some(std::path::Component::Normal(name)) => {
            pending.push(PathBuf::from(name));
            Dir::open_ambient_dir(Path::new("."), ambient_authority())?
        }
        Some(std::path::Component::ParentDir) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains an unsafe component",
            ));
        }
        None => Dir::open_ambient_dir(Path::new("."), ambient_authority())?,
    };
    pending.extend(components.filter_map(|component| match component {
        std::path::Component::Normal(name) => Some(PathBuf::from(name)),
        std::path::Component::CurDir
        | std::path::Component::RootDir
        | std::path::Component::Prefix(_) => None,
        std::path::Component::ParentDir => Some(PathBuf::new()),
    }));
    if pending.iter().any(|name| name.as_os_str().is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains an unsafe component",
        ));
    }
    let mut current = current;
    for name in pending {
        if create {
            match current.create_dir(&name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        current = current.open_dir_nofollow(&name)?;
    }
    Ok(current)
}

/// Opens the trusted root without following its endpoint or Unix path
/// components. Missing roots are created below the retained capability.
///
/// # Errors
/// Returns an I/O error when the root is a symlink/reparse point, cannot be
/// created, or its containing directory is unavailable.
pub fn open_trusted_root(path: &Path, create: bool) -> io::Result<Dir> {
    let root_path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    open_path_no_follow(root_path, create)
}
