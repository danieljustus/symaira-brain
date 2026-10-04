//! Brain's typed defaults/global/project/environment loader.
//! Pinned TOML syntax and diagnostics remain an explicit acceptance gate.
//! Stored `config get/set/path` remains a separate map API.
mod convert;
mod document;
mod error;
mod source;
mod value;
use crate::{GoText, go_path};
pub use convert::parse_bool;
pub use error::ConfigError;
pub use source::{ProcessSources, Sources};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
pub use value::{Audit, BrainConfig, Gateway, Modules, Patterns, Servers, UpdateCheck};

/// Returns `CoreKit` `DefaultPath` even without a usable HOME.
#[must_use]
pub fn default_path() -> PathBuf {
    default_path_with(&ProcessSources)
}
#[must_use]
pub fn default_path_with(source: &impl Sources) -> PathBuf {
    if let Some(xdg) = source.environment("XDG_CONFIG_HOME")
        && go_path::is_absolute(&go_path::os_bytes(&xdg), cfg!(windows))
    {
        return go_path::join(&[&xdg, OsStr::new("symbrain"), OsStr::new("config.toml")]);
    }
    let home = source
        .environment(go_path::home_variable())
        .filter(|home| !home.is_empty());
    let mut parts = Vec::new();
    if let Some(home) = home.as_deref() {
        parts.push(home);
    }
    parts.extend([
        OsStr::new(".config"),
        OsStr::new("symbrain"),
        OsStr::new("config.toml"),
    ]);
    go_path::join(&parts)
}
/// Loads a fresh resolved value at this invocation's Go admission boundary.
///
/// # Errors
/// Returns the first HOME, global, project or environment admission error.
pub fn load() -> Result<BrainConfig, ConfigError> {
    load_with(&ProcessSources)
}
/// Resolves an invocation through explicitly owned sources.
///
/// # Errors
/// Returns the first stage error, preserving its byte-valued context.
pub fn load_with(source: &impl Sources) -> Result<BrainConfig, ConfigError> {
    load_stages(source, None).map_err(|detail| wrap_error(&default_path_with(source), &detail))
}
/// Loads all stages with an injected global file (production uses `DefaultPath`).
///
/// # Errors
/// Returns the same ordered stage errors as [`load_with`].
pub fn load_with_global_path(
    global: &Path,
    source: &impl Sources,
) -> Result<BrainConfig, ConfigError> {
    load_stages(source, Some(global)).map_err(|detail| wrap_error(global, &detail))
}
fn wrap_error(global: &Path, detail: &GoText) -> ConfigError {
    let mut text = GoText::from("config: failed to load ");
    text.push(&go_path::os_bytes(global.as_os_str()));
    text.push(b": ");
    text.push(detail.as_ref());
    ConfigError(text)
}

fn load_stages(source: &impl Sources, global: Option<&Path>) -> Result<BrainConfig, GoText> {
    if source
        .environment(go_path::home_variable())
        .is_none_or(|home| home.is_empty())
    {
        let variable = if cfg!(windows) {
            "%userprofile%"
        } else {
            "$HOME"
        };
        return Err(format!("cannot determine home directory: {variable} is not defined").into());
    }
    let mut config = BrainConfig::default();
    let default_global;
    let global = if let Some(global) = global {
        global
    } else {
        default_global = default_path_with(source);
        &default_global
    };
    merge_file(&mut config, global, source)
        .map_err(|error| error.with_prefix("global config error: "))?;
    if let Ok(cwd) = source.current_directory() {
        let path = go_path::join(&[cwd.as_os_str(), OsStr::new(".symbrain.toml")]);
        merge_file(&mut config, &path, source)
            .map_err(|error| error.with_prefix("project config error: "))?;
    }
    convert::apply_environment(&mut config, source)
        .map_err(|error| error.with_prefix("env override error: "))?;
    if config.patterns.promotion_threshold <= 0 {
        config.patterns.promotion_threshold = 3;
    }
    Ok(config)
}
fn merge_file(config: &mut BrainConfig, path: &Path, source: &impl Sources) -> Result<(), GoText> {
    if source
        .metadata(path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    let bytes = source.read(path).map_err(|(operation, error)| {
        let mut text = GoText::from("failed to parse ");
        let raw_path = go_path::os_bytes(path.as_os_str());
        text.push(&raw_path);
        text.push(format!(": {operation} ").as_bytes());
        text.push(&raw_path);
        text.push(format!(": {}", error::cause(&error)).as_bytes());
        text
    })?;
    let document =
        document::parse(&bytes).map_err(|error| path_error("failed to parse ", path, &error))?;
    convert::apply_document(config, &document)
        .map_err(|error| path_error("failed to apply ", path, &error))
}
fn path_error(prefix: &str, path: &Path, error: &GoText) -> GoText {
    let mut text = GoText::from(prefix);
    text.push(&go_path::os_bytes(path.as_os_str()));
    text.push(b": ");
    text.push(error.as_ref());
    text
}

#[cfg(test)]
mod tests;
