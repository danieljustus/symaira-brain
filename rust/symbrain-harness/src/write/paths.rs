use cap_fs_ext::DirExt;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
#[cfg(unix)]
use cap_std::fs::{Permissions, PermissionsExt};
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn open_trusted_root(path: &Path, create: bool) -> io::Result<Dir> {
    let mut current = open_filesystem_anchor(path)?;
    for component in path.components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::CurDir | Component::RootDir | Component::Prefix(_) => continue,
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "trusted root must not contain a parent component",
                ));
            }
        };
        let created = if create {
            match current.create_dir(name) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        let next = current.open_dir_nofollow(name)?;
        if created {
            #[cfg(unix)]
            next.set_permissions(".", Permissions::from_mode(0o700))?;
        }
        current = next;
    }
    Ok(current)
}

#[cfg(unix)]
fn open_filesystem_anchor(path: &Path) -> io::Result<Dir> {
    let anchor = if path.is_absolute() {
        Path::new("/")
    } else {
        Path::new(".")
    };
    Dir::open_ambient_dir(anchor, ambient_authority())
}

#[cfg(windows)]
fn open_filesystem_anchor(path: &Path) -> io::Result<Dir> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => {
            if !matches!(components.next(), Some(Component::RootDir)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "trusted root must be an absolute Windows path",
                ));
            }
            let mut anchor = prefix.as_os_str().to_os_string();
            anchor.push("\\");
            Dir::open_ambient_dir(PathBuf::from(anchor), ambient_authority())
        }
        Some(Component::RootDir) => Dir::open_ambient_dir(Path::new("\\"), ambient_authority()),
        _ => Dir::open_ambient_dir(Path::new("."), ambient_authority()),
    }
}

pub(super) fn open_parent_capability(root: &Dir, parent: &Path, create: bool) -> io::Result<Dir> {
    let mut current = root.try_clone()?;
    for component in parent.components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::CurDir => continue,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "configuration target parent is not relative",
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
            #[cfg(unix)]
            next.set_permissions(".", Permissions::from_mode(0o700))?;
        }
        current = next;
    }
    Ok(current)
}

pub(super) fn split_atomic_target(relative_target: &Path) -> io::Result<(PathBuf, PathBuf)> {
    if relative_target.as_os_str().is_empty() || relative_target.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration target must be relative",
        ));
    }
    let components = relative_target.components().collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            component,
            Component::RootDir | Component::Prefix(_) | Component::ParentDir
        )
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration target must stay beneath its trusted root",
        ));
    }
    let name = relative_target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration target has no file name",
        )
    })?;
    let mut parent = PathBuf::new();
    for component in &components[..components.len() - 1] {
        if let Component::Normal(component) = component {
            parent.push(component);
        }
    }
    if parent.as_os_str().is_empty() {
        parent.push(".");
    }
    Ok((parent, PathBuf::from(name)))
}
