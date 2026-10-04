//! Per-call sources preserve Go's stage ordering; no global cache.
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Environment, cwd and file admission can be injected without process globals.
pub trait Sources {
    fn environment(&self, name: &str) -> Option<OsString>;
    /// # Errors
    /// Returns the actual cwd failure; the loader skips its project stage.
    fn current_directory(&self) -> io::Result<PathBuf>;
    /// # Errors
    /// Returns the actual stat failure; only `NotFound` suppresses admission.
    fn metadata(&self, path: &Path) -> io::Result<()>;
    /// # Errors
    /// Returns the actual open/read operation and OS error.
    fn read(&self, path: &Path) -> Result<Vec<u8>, (&'static str, io::Error)>;
}
#[derive(Debug, Clone, Copy)]
pub struct ProcessSources;
impl Sources for ProcessSources {
    fn environment(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
    fn current_directory(&self) -> io::Result<PathBuf> {
        // Go os.Getwd admits an absolute PWD only when it names the same
        // current directory. Keep that spelling until lexical filepath.Join;
        // following its links for owner selection would choose another file.
        #[cfg(unix)]
        if let Some(pwd) = std::env::var_os("PWD") {
            use std::os::unix::fs::MetadataExt;
            if crate::go_path::os_bytes(&pwd).starts_with(b"/") {
                let dot = std::fs::metadata(".")?;
                if let Ok(candidate) = std::fs::metadata(&pwd)
                    && dot.dev() == candidate.dev()
                    && dot.ino() == candidate.ino()
                {
                    return Ok(PathBuf::from(pwd));
                }
            }
        }
        // Go Windows explicitly ignores PWD and calls the platform Getwd.
        std::env::current_dir()
    }
    fn metadata(&self, path: &Path) -> io::Result<()> {
        std::fs::metadata(path).map(|_| ())
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>, (&'static str, io::Error)> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Go permits read-only directory handles, shares read/write but not
            // delete, then reports the actual read error (no metadata surrogate).
            options.share_mode(3).custom_flags(0x0200_0000);
        }
        let mut file = options.open(path).map_err(|error| ("open", error))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| ("read", error))?;
        Ok(bytes)
    }
}
