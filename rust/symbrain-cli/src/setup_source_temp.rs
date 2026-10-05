//! Separate control capture from the unchanged worker staging/temp contract.
use std::path::{Path, PathBuf};
use symbrain_managed::{GoText, format_io_error};

pub(super) fn capture() -> Result<tempfile::NamedTempFile, String> {
    let requested = std::env::temp_dir();
    let requested = if requested.is_absolute() {
        requested
    } else {
        std::env::current_dir()
            .map_err(|error| format!("resolve control capture root: {error}"))?
            .join(requested)
    };
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
    cleanup: PathBuf,
}
impl Stage {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cleanup);
    }
}

pub(super) fn stage(parent: &Path) -> Result<Stage, GoText> {
    let mut detail = None;
    // Builder supplies random names and collision retries; Stage alone owns
    // directory cleanup (the generic TempPath file cleanup is disabled).
    tempfile::Builder::new()
        .prefix("symbrain-source-build-")
        .disable_cleanup(true)
        .make_in(parent, |absolute| {
            // tempfile chooses an absolute random name; Go keeps a relative
            // worker temp path relative in compiler argv. Cleanup owns the
            // absolute identity independently of that observable path.
            let path = if parent.is_absolute() {
                absolute.to_owned()
            } else {
                parent.join(absolute.file_name().expect("generated stage filename"))
            };
            #[cfg(unix)]
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => Ok(Stage {
                    path,
                    cleanup: absolute.to_owned(),
                }),
                Err(error) => {
                    detail = Some(if error.kind() == std::io::ErrorKind::NotFound {
                        match std::fs::metadata(parent) {
                            Err(stat) if stat.kind() == std::io::ErrorKind::NotFound => {
                                let display_parent = if cfg!(windows) {
                                    super::absolute(parent.as_os_str())
                                        .unwrap_or_else(|_| parent.to_path_buf())
                                } else {
                                    parent.to_path_buf()
                                };
                                let operation = if cfg!(windows) {
                                    "GetFileAttributesEx "
                                } else {
                                    "stat "
                                };
                                GoText::path(
                                    operation,
                                    &display_parent,
                                    &format!(": {}", format_io_error(&stat)),
                                )
                            }
                            _ => GoText::path(
                                "mkdir ",
                                &path,
                                &format!(": {}", format_io_error(&error)),
                            ),
                        }
                    } else {
                        GoText::path("mkdir ", &path, &format!(": {}", format_io_error(&error)))
                    });
                    Err(error)
                }
            }
        })
        .map(|temp| temp.into_parts().0)
        .map_err(|error| detail.unwrap_or_else(|| format_io_error(&error).into()))
}
