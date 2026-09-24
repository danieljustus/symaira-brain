use crate::FirefoxError;
use std::path::{Path, PathBuf};

pub fn resolve_firefox_executable(explicit: Option<&Path>) -> Result<PathBuf, FirefoxError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_owned());
        }
        return Err(FirefoxError::Driver(format!(
            "Firefox executable not found at {}",
            path.display()
        )));
    }
    let candidates = if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/Firefox.app/Contents/MacOS/firefox",
        )]
    } else if cfg!(windows) {
        vec![PathBuf::from(
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
        )]
    } else {
        vec![
            PathBuf::from("/usr/bin/firefox"),
            PathBuf::from("/usr/lib/firefox/firefox"),
        ]
    };
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| FirefoxError::Driver("Firefox executable is unavailable".into()))
}
