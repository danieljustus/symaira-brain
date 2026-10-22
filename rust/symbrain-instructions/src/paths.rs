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

/// Lexical equivalent of Go's `filepath.Clean`: no filesystem access, `.` and
/// empty components dropped, `..` folded into a preceding real component,
/// leading `..` kept, `..` at a root dropped, and an empty result spelled `.`.
#[must_use]
pub fn clean_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match result.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    result.pop();
                }
                Some(std::path::Component::ParentDir) | None => result.push(".."),
                _ => {}
            },
            component => result.push(component.as_os_str()),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

/// Go's `os.Getwd`: `$PWD` when it names the current directory, otherwise the
/// kernel's answer.
#[cfg(unix)]
fn go_getwd() -> io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    if let Some(pwd) = std::env::var_os("PWD").map(PathBuf::from)
        && pwd.is_absolute()
        && let (Ok(dot), Ok(named)) = (std::fs::metadata("."), std::fs::metadata(&pwd))
        && dot.dev() == named.dev()
        && dot.ino() == named.ino()
    {
        return Ok(pwd);
    }
    std::env::current_dir()
}

/// Go's `filepath.Abs(filepath.Clean(path))`, which `openDirectoryNoFollow`
/// applies before it walks the trusted root from `/`.
#[cfg(unix)]
fn go_abs_clean(path: &Path) -> io::Result<PathBuf> {
    let cleaned = clean_path(path);
    if cleaned.is_absolute() {
        return Ok(cleaned);
    }
    Ok(clean_path(&go_getwd()?.join(cleaned)))
}

/// Opens a path component-by-component without following symlinks/reparse
/// points. Missing components are created only when requested.
///
/// Like Go, the path is first made absolute and lexically cleaned, so `..`
/// and `.` components never reach the component walk.
#[cfg(unix)]
fn open_path_no_follow(path: &Path, create: bool) -> io::Result<Dir> {
    let path = physical_path(&go_abs_clean(path)?);
    let mut current = Dir::open_ambient_dir(Path::new("/"), ambient_authority())?;
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
