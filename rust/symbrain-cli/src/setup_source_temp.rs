//! Separate control capture from the unchanged worker staging/temp contract.
use std::path::{Path, PathBuf};
use symbrain_managed::format_io_error;

pub(super) fn capture() -> Result<tempfile::NamedTempFile, String> {
    let requested = std::env::temp_dir();
    // A malformed worker temp root must fail at per-module staging, after Git.
    // Only the private control capture uses its nearest existing directory;
    // no worker environment, cache, staging root or directory is substituted.
    let parent = requested
        .ancestors()
        .find(|parent| parent.is_dir())
        .ok_or_else(|| "no existing control capture ancestor".to_owned())?;
    tempfile::Builder::new()
        .prefix(".symbrain-source-capture-")
        .tempfile_in(parent)
        .map_err(|error| format!("create build capture: {error}"))
}

pub(super) struct Stage {
    path: PathBuf,
}
impl Stage {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(super) fn stage(parent: &Path) -> Result<Stage, String> {
    let mut detail = None;
    // Builder supplies random names and collision retries; Stage alone owns
    // directory cleanup (the generic TempPath file cleanup is disabled).
    tempfile::Builder::new()
        .prefix("symbrain-source-build-")
        .disable_cleanup(true)
        .make_in(parent, |path| {
            #[cfg(unix)]
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(path) {
                Ok(()) => Ok(Stage {
                    path: path.to_owned(),
                }),
                Err(error) => {
                    detail = Some(if error.kind() == std::io::ErrorKind::NotFound {
                        match std::fs::metadata(parent) {
                            Err(stat) if stat.kind() == std::io::ErrorKind::NotFound => {
                                format!("stat {}: {}", parent.display(), format_io_error(&stat))
                            }
                            _ => format!("mkdir {}: {}", path.display(), format_io_error(&error)),
                        }
                    } else {
                        format!("mkdir {}: {}", path.display(), format_io_error(&error))
                    });
                    Err(error)
                }
            }
        })
        .map(|temp| temp.into_parts().0)
        .map_err(|error| detail.unwrap_or_else(|| format_io_error(&error)))
}
