//! Additive filesystem-native discovery keeps configured and managed owners raw.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use symbrain_core::{GoText, config::format_go_quoted_bytes, go_path};

#[derive(Debug)]
pub struct PathDiscoveryError {
    pub kind: std::io::ErrorKind,
    pub text: GoText,
}
impl std::fmt::Display for PathDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.text, f)
    }
}
impl std::error::Error for PathDiscoveryError {}

/// Resolves a child without converting its filesystem selector to Unicode.
///
/// # Errors
/// Returns raw configured-path admission or missing-child diagnostics.
pub fn discover_path(binary: &str, override_path: &OsStr) -> Result<PathBuf, PathDiscoveryError> {
    if !override_path.is_empty() {
        let path = Path::new(override_path);
        if super::client::is_usable_executable(path) {
            return Ok(path.to_path_buf());
        }
        let quoted = format_go_quoted_bytes(&go_path::os_bytes(override_path));
        let prefix = format!("broker: configured binary_path {quoted} for {binary:?}");
        return Err(match std::fs::metadata(path) {
            Err(error) => {
                let literal = error.to_string();
                let cause = error.raw_os_error().map_or_else(
                    || literal.clone(),
                    |code| {
                        let message = literal
                            .strip_suffix(&format!(" (os error {code})"))
                            .unwrap_or(&literal);
                        if cfg!(unix) {
                            message.to_lowercase()
                        } else {
                            message.to_owned()
                        }
                    },
                );
                PathDiscoveryError {
                    kind: error.kind(),
                    text: GoText::path(&format!("{prefix}: stat "), path, &format!(": {cause}")),
                }
            }
            Ok(_) => PathDiscoveryError {
                kind: std::io::ErrorKind::PermissionDenied,
                text: format!("{prefix} is not an executable regular file").into(),
            },
        });
    }
    if let Some(home) = std::env::var_os(go_path::home_variable()).filter(|home| !home.is_empty()) {
        let directory = go_path::join(&[&home, OsStr::new(".symaira"), OsStr::new("bin")]);
        if let Some(path) = super::client::find_in_dir(&directory, binary) {
            return Ok(path);
        }
    }
    if let Some(paths) = std::env::var_os("PATH").filter(|paths| !paths.is_empty()) {
        if let Some(path) = std::env::split_paths(&paths)
            .find_map(|directory| super::client::find_in_dir(&directory, binary))
        {
            return Ok(path);
        }
    }
    let path_var = if cfg!(windows) { "%PATH%" } else { "$PATH" };
    Err(PathDiscoveryError { kind: std::io::ErrorKind::NotFound,
        text: format!("broker: {binary:?} not found on PATH or in managed directory: exec: {binary:?}: executable file not found in {path_var}").into() })
}
