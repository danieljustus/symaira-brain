#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::PermissionsExt as CapPermissionsExt;
use cap_std::fs::{Dir, OpenOptions, Permissions};
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use super::atomic::{cleanup_file, join_cleanup_error, reserve_name};
use super::paths::{open_parent_capability, open_trusted_root, split_atomic_target};
use super::replace::replace_existing;
use super::{MAX_CONFIG_BYTES, MAX_TEMP_ATTEMPTS};

/// A regular file snapshot read through one no-follow file handle.
#[derive(Debug)]
pub struct FileSnapshot {
    /// Bytes read from the opened file.
    pub bytes: Vec<u8>,
    /// Permissions observed on the same opened file handle.
    pub permissions: Permissions,
}

/// A capability to one validated target and its containing directory.
///
/// The directory handle is retained from resolution through read, backup,
/// temporary creation, write, replace, cleanup, and directory synchronization.
pub struct AtomicFile {
    pub(super) parent: Dir,
    pub(super) name: PathBuf,
    display_path: PathBuf,
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
        display_path: PathBuf,
        create_parent: bool,
    ) -> io::Result<Self> {
        let (parent_path, name) = split_atomic_target(relative_target)?;
        let root_path = if trusted_root.as_os_str().is_empty() {
            Path::new(".")
        } else {
            trusted_root
        };
        let root = open_trusted_root(root_path, create_parent)?;
        let parent = open_parent_capability(&root, &parent_path, create_parent)?;
        Ok(Self {
            parent,
            name,
            display_path,
        })
    }

    /// Reads the target from this capability, rejecting symlinks and special files.
    ///
    /// Metadata and bytes come from the same securely opened handle, so a path
    /// replacement between a metadata probe and the read cannot redirect the
    /// snapshot to another file.
    ///
    /// # Errors
    /// Returns an I/O error when the target is absent, unsafe, unreadable, or
    /// larger than [`MAX_CONFIG_BYTES`].
    pub fn read_snapshot(&self) -> io::Result<FileSnapshot> {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = self.parent.open_with(&self.name, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration target is not a regular file",
            ));
        }
        let capacity = usize::try_from(metadata.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "configuration target is too large",
            )
        })?;
        let mut bytes = Vec::with_capacity(capacity.min(MAX_CONFIG_BYTES));
        file.take((MAX_CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "configuration target exceeds maximum size",
            ));
        }
        Ok(FileSnapshot {
            bytes,
            permissions: metadata.permissions(),
        })
    }

    /// Checks whether the target directory entry exists without following a symlink.
    ///
    /// This lets removal accept symlinks and special files while keeping the
    /// final mutation no-follow and scoped to the retained parent capability.
    ///
    /// # Errors
    /// Returns an I/O error when the directory entry cannot be inspected.
    pub fn exists_no_follow(&self) -> io::Result<bool> {
        match self.parent.symlink_metadata(&self.name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Removes the target directory entry without following a symlink.
    ///
    /// The parent directory capability is retained from resolution through the
    /// final operation. Regular files, symlinks, and special files use
    /// `remove_file`; an empty directory uses `remove_dir`, matching Go's
    /// `os.Remove` while preventing a symlink replacement from redirecting the
    /// deletion outside the trusted parent.
    ///
    /// # Errors
    /// Returns an I/O error when the directory entry cannot be removed.
    pub fn remove_no_follow(&self) -> io::Result<bool> {
        let metadata = match self.parent.symlink_metadata(&self.name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if metadata.is_dir() {
            self.parent.remove_dir(&self.name)?;
        } else {
            self.parent.remove_file(&self.name)?;
        }
        #[cfg(unix)]
        self.sync_parent()?;
        Ok(true)
    }

    /// Returns the user-visible path associated with this capability.
    #[must_use]
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    /// Creates a collision-safe timestamped backup from a prior snapshot.
    ///
    /// The first backup keeps the Go-visible `*.bak.<timestamp>` spelling.
    /// If that name already exists, numeric suffixes are appended without
    /// overwriting any earlier rollback point.
    ///
    /// # Errors
    /// Returns an I/O error if a unique backup cannot be created durably.
    pub fn backup_snapshot(&self, snapshot: &FileSnapshot, timestamp: &str) -> io::Result<PathBuf> {
        let base = self.name.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration target has no file name",
            )
        })?;
        for suffix in 0_u32..MAX_TEMP_ATTEMPTS {
            let backup_name = backup_name(base, timestamp, suffix);
            if !reserve_name(&self.parent, &backup_name)? {
                continue;
            }
            let result = self.write_reserved_backup(&backup_name, snapshot);
            match result {
                Ok(()) => {
                    #[cfg(unix)]
                    self.sync_parent()?;
                    return Ok(self.display_path.with_file_name(backup_name));
                }
                Err(error) => {
                    return match join_cleanup_error(
                        Err(error),
                        cleanup_file(&self.parent, &backup_name),
                        "backup cleanup",
                    ) {
                        Ok(()) => Err(io::Error::other("backup failed without an error")),
                        Err(error) => Err(error),
                    };
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "unable to allocate unique backup file",
        ))
    }

    fn write_reserved_backup(&self, backup_name: &Path, snapshot: &FileSnapshot) -> io::Result<()> {
        let (temporary_name, mut temporary) = self.create_temp()?;
        let result = (|| {
            temporary.set_permissions(snapshot.permissions.clone())?;
            temporary.write_all(&snapshot.bytes)?;
            temporary.sync_all()?;
            drop(temporary);
            replace_existing(&self.parent, &temporary_name, backup_name)?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(error) => join_cleanup_error(
                Err(error),
                cleanup_file(&self.parent, &temporary_name),
                "temporary backup cleanup",
            ),
        }
    }

    /// Atomically replaces the target with private-mode bytes.
    ///
    /// The target is never followed as a symlink. Temporary files are created
    /// with `0600`, synced before replacement, and removed on every failure.
    ///
    /// # Errors
    /// Returns an I/O error for unsafe targets, failed writes, replacement,
    /// cleanup, or durability synchronization.
    pub fn write(&self, data: &[u8], mode: u32) -> io::Result<()> {
        #[cfg(not(unix))]
        let _ = mode;
        if data.len() > MAX_CONFIG_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "configuration output exceeds maximum size",
            ));
        }
        match self.parent.symlink_metadata(&self.name) {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "configuration target is not a regular file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let (temporary_name, mut temporary) = self.create_temp()?;
        let mut replaced = false;
        let result = (|| {
            #[cfg(unix)]
            temporary.set_permissions(Permissions::from_mode(mode))?;
            #[cfg(not(unix))]
            {
                let mut permissions = temporary.metadata()?.permissions();
                permissions.set_readonly(false);
                temporary.set_permissions(permissions)?;
            }
            temporary.write_all(data)?;
            temporary.sync_all()?;
            drop(temporary);
            replace_existing(&self.parent, &temporary_name, &self.name)?;
            replaced = true;
            #[cfg(unix)]
            self.sync_parent()?;
            Ok(())
        })();
        if !replaced {
            return join_cleanup_error(
                result,
                cleanup_file(&self.parent, &temporary_name),
                "temporary file cleanup",
            );
        }
        result
    }

    fn create_temp(&self) -> io::Result<(PathBuf, cap_std::fs::File)> {
        let base = self.name.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration target has no file name",
            )
        })?;
        for suffix in 0_u32..MAX_TEMP_ATTEMPTS {
            let name = temporary_name(base, std::process::id(), suffix);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match self.parent.open_with(&name, &options) {
                Ok(file) => return Ok((name, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "unable to allocate atomic temp file",
        ))
    }

    #[cfg(unix)]
    fn sync_parent(&self) -> io::Result<()> {
        sync_directory(&self.parent)
    }
}
#[cfg(unix)]
fn sync_directory(parent: &Dir) -> io::Result<()> {
    use rustix::fs::{self, OFlags};

    // cap-std uses O_PATH for directory capabilities on Linux, and Linux
    // rejects fsync(O_PATH). Reopen through the retained directory fd so the
    // sync remains capability-scoped instead of falling back to a pathname.
    let fd = fs::openat(
        parent.as_fd(),
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        fs::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    fs::fsync(&fd).map_err(io::Error::from)
}

pub(super) fn backup_name(base: &OsStr, timestamp: &str, suffix: u32) -> PathBuf {
    let mut name = base.to_os_string();
    name.push(".bak.");
    name.push(timestamp);
    if suffix != 0 {
        name.push(format!(".{suffix}"));
    }
    PathBuf::from(name)
}

pub(super) fn temporary_name(base: &OsStr, process_id: u32, suffix: u32) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(base);
    name.push(".symbrain-tmp-");
    name.push(process_id.to_string());
    if suffix != 0 {
        name.push(format!("-{suffix}"));
    }
    PathBuf::from(name)
}

#[cfg(all(test, unix))]
mod tests {
    use super::backup_name;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    #[test]
    fn backup_name_preserves_non_utf8_basename_bytes() {
        let base = std::ffi::OsString::from_vec(b"config-\xff.json".to_vec());
        let name = backup_name(&base, "20260906T200001Z", 0);
        assert_eq!(
            name.as_os_str().as_bytes(),
            b"config-\xff.json.bak.20260906T200001Z"
        );
    }
}
