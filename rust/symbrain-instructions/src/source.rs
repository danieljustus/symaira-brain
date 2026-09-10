use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, OpenOptions};
use std::env;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::paths::open_source_parent;
use super::{
    APP_NAME, GLOBAL_FILE_NAME, MAX_SOURCE_FILE_BYTES, MAX_SOURCE_TOTAL_BYTES, PROJECT_DIR_NAME,
    PROJECT_FILE_NAME,
};

/// Resolves the global instruction file using XDG config semantics.
///
/// An absolute `xdg_config_home` is preferred. Otherwise the home directory
/// is used with `.config`; when neither is available, the relative fallback
/// `.config/symbrain/instructions.md` matches the Go path helper's fallback.
#[must_use]
pub fn resolve_global_path(xdg_config_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    let base = xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| {
            home.filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join(APP_NAME).join(GLOBAL_FILE_NAME)
}

struct SourceLocation {
    path: PathBuf,
    capability: Option<SourceCapability>,
    error: Option<String>,
}

struct SourceCapability {
    parent: Dir,
    name: PathBuf,
}

/// A pair of global and optional project-local canonical instruction files.
pub struct Source {
    /// Resolved global instruction path.
    pub global_path: PathBuf,
    /// Resolved project-local instruction path, if a project was supplied.
    pub project_path: Option<PathBuf>,
    global: SourceLocation,
    project: Option<SourceLocation>,
}

impl Source {
    /// Resolve paths from the current process environment.
    #[must_use]
    pub fn new(project_dir: Option<&Path>) -> Self {
        let xdg = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = if cfg!(windows) {
            env::var_os("USERPROFILE").map(PathBuf::from)
        } else {
            env::var_os("HOME").map(PathBuf::from)
        };
        Self::with_environment(project_dir, xdg.as_deref(), home.as_deref())
    }

    /// Resolve paths from explicit XDG and home values without reading or
    /// changing process environment.
    #[must_use]
    pub fn with_environment(
        project_dir: Option<&Path>,
        xdg_config_home: Option<&Path>,
        home: Option<&Path>,
    ) -> Self {
        let global_path = resolve_global_path(xdg_config_home, home);
        let project_path =
            project_dir.map(|dir| dir.join(PROJECT_DIR_NAME).join(PROJECT_FILE_NAME));
        Self::from_paths(global_path, project_path)
    }

    /// Construct a source from explicit file paths, primarily for isolated
    /// callers and tests. Parent capabilities are opened without following
    /// symlinks or reparse points and retained for the source lifetime.
    #[must_use]
    pub fn from_paths(global_path: PathBuf, project_path: Option<PathBuf>) -> Self {
        let global = source_location(global_path.clone());
        let project = project_path
            .as_ref()
            .map(|path| source_location(path.clone()));
        Self {
            global_path,
            project_path,
            global,
            project,
        }
    }

    /// Read global content followed directly by project content.
    ///
    /// Missing files are ignored. Existing files are returned byte-for-byte;
    /// no separator, newline, UTF-8 conversion, or normalization is added.
    /// Each file is opened once through a retained capability-rooted directory,
    /// checked on that handle, bounded, and then read from that same handle.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error when an existing source cannot be read,
    /// or when a source is not a bounded regular file.
    pub fn content(&self) -> io::Result<Vec<u8>> {
        let mut merged = Vec::new();
        append_location(&mut merged, &self.global)?;
        if let Some(project) = &self.project {
            append_location(&mut merged, project)?;
        }
        Ok(merged)
    }
}

fn source_location(path: PathBuf) -> SourceLocation {
    let parent_path = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().map(PathBuf::from);
    let (capability, error) = match (name, open_source_parent(parent_path)) {
        (Some(name), Ok(parent)) => (Some(SourceCapability { parent, name }), None),
        (_, Err(error)) if error.kind() == io::ErrorKind::NotFound => (None, None),
        (_, Err(error)) => (None, Some(error.to_string())),
        (None, Ok(_)) => (None, Some("instruction path has no file name".into())),
    };
    SourceLocation {
        path,
        capability,
        error,
    }
}

fn append_location(merged: &mut Vec<u8>, location: &SourceLocation) -> io::Result<()> {
    if let Some(error) = &location.error {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "instructions: secure source parent {}: {error}",
                location.path.display()
            ),
        ));
    }
    let Some(capability) = &location.capability else {
        return Ok(());
    };
    let bytes = match read_capability(capability) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("instructions: read {}: {error}", location.path.display()),
            ));
        }
    };
    if (merged.len() as u64) > MAX_SOURCE_TOTAL_BYTES.saturating_sub(bytes.len() as u64) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "instructions: merged source exceeds maximum size of {MAX_SOURCE_TOTAL_BYTES} bytes"
            ),
        ));
    }
    merged.extend_from_slice(&bytes);
    Ok(())
}

fn read_capability(capability: &SourceCapability) -> io::Result<Vec<u8>> {
    let metadata = capability.parent.symlink_metadata(&capability.name)?;
    if metadata.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is a symlink", capability.name.display()),
        ));
    }
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a regular file", capability.name.display()),
        ));
    }
    if metadata.len() > MAX_SOURCE_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} exceeds maximum size of {MAX_SOURCE_FILE_BYTES} bytes",
                capability.name.display()
            ),
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = capability.parent.open_with(&capability.name, &options)?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("instruction source exceeds {MAX_SOURCE_FILE_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}
