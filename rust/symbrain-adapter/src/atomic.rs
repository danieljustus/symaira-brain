use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
#[cfg(windows)]
use cap_std::fs::OpenOptionsExt;
#[cfg(unix)]
use cap_std::fs::Permissions;
use cap_std::fs::{Dir, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, SYNCHRONIZE, WRITE_DAC, WRITE_OWNER,
};

#[cfg(unix)]
use super::metadata_unix;
#[cfg(windows)]
use super::metadata_windows;
use super::path::validate_relative_target_path;

/// A capability to one validated target and its containing directory.
///
/// The directory handle is retained from resolution through read, temporary
/// creation, write, rename, cleanup, and directory synchronization.
pub struct AtomicFile {
    parent: Dir,
    name: PathBuf,
}

impl AtomicFile {
    /// Opens a trusted root and resolves a project-relative target.
    ///
    /// Missing parents are created only when `create_parent` is true. Parent
    /// components are opened without following symlinks.
    ///
    /// # Errors
    /// Returns an error for an invalid target, missing parent, or symlinked
    /// parent component.
    pub fn open(
        trusted_root: &Path,
        relative_target: &Path,
        create_parent: bool,
    ) -> io::Result<Self> {
        let (parent_path, name) = split_atomic_target(relative_target)?;
        let root = symbrain_instructions::open_trusted_root(trusted_root, create_parent)?;
        let parent = open_parent_capability(&root, &parent_path, create_parent)?;
        Ok(Self { parent, name })
    }

    /// Reads the target from this capability, rejecting symlinks and special files.
    ///
    /// # Errors
    /// Returns an I/O error when the target is absent, unsafe, unreadable, or
    /// larger than the instruction source limit.
    pub fn read_bounded(&self) -> io::Result<Vec<u8>> {
        let metadata = self.parent.symlink_metadata(&self.name)?;
        if metadata.is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "adapter target is not a regular file",
            ));
        }
        if metadata.len() > 1 << 20 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "adapter target exceeds maximum size",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = self.parent.open_with(&self.name, &options)?;
        let opened_metadata = file.metadata()?;
        if opened_metadata.is_symlink() || !opened_metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "adapter target is not a regular file",
            ));
        }
        let capacity = usize::try_from(metadata.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "adapter target is too large")
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take((1 << 20) + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 1 << 20 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "adapter target exceeds maximum size",
            ));
        }
        Ok(bytes)
    }

    /// Atomically replaces the target through this capability.
    ///
    /// Existing regular-file permissions are preserved; new files use `0600`
    /// on Unix. Temporary files are removed on every failure before rename.
    ///
    /// # Errors
    /// Returns an I/O error for unsafe targets, failed writes, renames, or
    /// durability synchronization.
    #[allow(clippy::too_many_lines)]
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        let existing_metadata = match self.parent.symlink_metadata(&self.name) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(metadata) = &existing_metadata
            && (metadata.is_symlink() || !metadata.is_file())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "adapter target is not a regular file",
            ));
        }
        let base = self.name.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "adapter target has no file name",
            )
        })?;
        let mut temporary_name = PathBuf::new();
        let mut temporary = None;
        for suffix in 0_u32..1000 {
            temporary_name = PathBuf::from(format!(
                ".{}.symbrain-tmp-{}{}",
                base.to_string_lossy(),
                std::process::id(),
                if suffix == 0 {
                    String::new()
                } else {
                    format!("-{suffix}")
                }
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            #[cfg(windows)]
            options.access_mode(
                FILE_GENERIC_READ
                    | FILE_GENERIC_WRITE
                    | DELETE
                    | SYNCHRONIZE
                    | WRITE_DAC
                    | WRITE_OWNER,
            );
            match self.parent.open_with(&temporary_name, &options) {
                Ok(file) => {
                    temporary = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let mut temporary = temporary.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "unable to allocate atomic temp file",
            )
        })?;
        let mut renamed = false;
        let result = (|| {
            if let Some(metadata) = &existing_metadata {
                temporary.set_permissions(metadata.permissions())?;
                #[cfg(unix)]
                metadata_unix::copy(&self.parent, &self.name, &temporary)?;
                #[cfg(windows)]
                metadata_windows::copy(&self.parent, &self.name, &temporary)?;
            } else {
                #[cfg(unix)]
                {
                    use cap_std::fs::PermissionsExt;
                    temporary.set_permissions(Permissions::from_mode(0o600))?;
                }
                #[cfg(not(unix))]
                {
                    let mut permissions = temporary.metadata()?.permissions();
                    permissions.set_readonly(false);
                    temporary.set_permissions(permissions)?;
                }
            }
            temporary.write_all(data)?;
            if existing_metadata.is_some() {
                // Writing can clear Unix set-id bits. Reapply the complete
                // source metadata after the bytes are written as well as
                // before, while retaining the same no-follow source handle
                // boundary and without ever mutating the source inode.
                #[cfg(unix)]
                metadata_unix::copy(&self.parent, &self.name, &temporary)?;
                #[cfg(windows)]
                metadata_windows::copy(&self.parent, &self.name, &temporary)?;
            }
            temporary.sync_all()?;
            drop(temporary);
            self.parent
                .rename(&temporary_name, &self.parent, &self.name)?;
            renamed = true;
            #[cfg(not(windows))]
            self.parent.try_clone()?.into_std_file().sync_all()?;
            Ok(())
        })();
        if !renamed {
            let _ = self.parent.remove_file(&temporary_name);
        }
        result
    }
}

fn open_parent_capability(root: &Dir, parent: &Path, create: bool) -> io::Result<Dir> {
    let mut current = root.try_clone()?;
    for component in parent.components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::CurDir => continue,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "adapter target parent is not relative",
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
            {
                use cap_std::fs::PermissionsExt;
                next.set_permissions(".", Permissions::from_mode(0o700))?;
            }
        }
        current = next;
    }
    Ok(current)
}

/// Atomically writes a validated target beneath a trusted root.
///
/// # Errors
/// Returns an I/O error for unsafe paths, failed writes, or failed syncing.
pub fn write_atomic(trusted_root: &Path, relative_target: &Path, data: &[u8]) -> io::Result<()> {
    AtomicFile::open(trusted_root, relative_target, true)?.write(data)
}

fn split_atomic_target(relative_target: &Path) -> io::Result<(PathBuf, PathBuf)> {
    validate_relative_target_path(relative_target)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let raw = relative_target.to_string_lossy();
    let parts = raw
        .split(['/', '\\'])
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let name = parts.last().cloned().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "adapter target has no file name",
        )
    })?;
    let mut parent = PathBuf::new();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
    }
    if parent.as_os_str().is_empty() {
        parent.push(".");
    }
    Ok((parent, name))
}
