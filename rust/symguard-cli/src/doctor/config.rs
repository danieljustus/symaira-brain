//! Guard doctor configuration parsing and compatibility gate.
use std::io;
use std::path::{Path, PathBuf};
use std::{env, fs};
use symbrain_core::GoText;
use toml_edit::Document;
#[cfg(test)]
use toml_edit::DocumentMut;

#[path = "config_decode.rs"]
mod decode;
use decode::decode_config;
#[path = "config_warnings.rs"]
mod warnings;

/// Go configuration path precedence: explicit override, XDG, then home.
pub(super) fn config_path() -> PathBuf {
    if let Some(env) = env::var_os("SYMGUARD_CONFIG").filter(|v| !v.is_empty()) {
        return PathBuf::from(env);
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return crate::guard_scan::join_native_path(Path::new(&xdg), &["symguard", "config.toml"]);
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    // filepath.Join cleans lexically before the OS can resolve a symlink/.. pair.
    // Explicit SYMGUARD_CONFIG above deliberately keeps the caller's raw path.
    crate::guard_scan::join_native_path(&home, &[".config", "symguard", "config.toml"])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpawnEntry {
    pub(super) path: String,
    pub(super) argv_prefix: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct LoadedConfig {
    pub(super) rules: usize,
    pub(super) allowlist: Vec<SpawnEntry>,
}

/// The outcomes of `config.Load()` that doctor can reproduce natively.
pub(super) enum ConfigState {
    Missing,
    Loaded(LoadedConfig),
    ParseError(String),
    ValidationError(String),
}

pub(super) struct ConfigOutcome {
    pub(super) state: ConfigState,
    pub(super) warnings: Vec<GoText>,
}

impl ConfigOutcome {
    fn silent(state: ConfigState) -> Self {
        Self {
            state,
            warnings: Vec::new(),
        }
    }
}

/// Ports proven `config.Load()` states. `None` means a decoder diagnostic,
/// type or map-order-dependent failure remains Go-owned. Warnings are returned
/// only after typed decoding and buffered until the whole doctor is admitted.
pub(super) fn load_config(path: &Path) -> Option<ConfigOutcome> {
    // File selection preserves Windows UTF16, but Core GoText's Windows byte
    // projection is not yet WTF8-exact. Delegate before reading/emitting text.
    #[cfg(windows)]
    path.to_str()?;
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Some(ConfigOutcome::silent(ConfigState::Missing));
        }
        Err(_) => return None,
    };
    let doc = match Document::parse(text.as_str()) {
        Ok(doc) => doc,
        Err(error) => {
            let diagnostic = go_missing_equals_diagnostic(&text, &error)?;
            return Some(ConfigOutcome::silent(ConfigState::ParseError(format!(
                "toml: {diagnostic}"
            ))));
        }
    };
    let decoded = decode_config(doc.as_table())?;
    let warnings = warnings::collect(doc.as_table(), path)?;
    let state = match decoded.validate()? {
        Ok(loaded) => ConfigState::Loaded(loaded),
        Err(error) => ConfigState::ValidationError(error),
    };
    Some(ConfigOutcome { state, warnings })
}

/// Maps only `toml_edit`'s missing-`=` error with a printable ASCII offender;
/// all other parser messages stay on the Go fallback path.
pub(super) fn go_missing_equals_diagnostic(
    text: &str,
    error: &toml_edit::TomlError,
) -> Option<String> {
    if error.message() != "key with no value, expected `=`" {
        return None;
    }
    let span = error.span()?;
    if !span.is_empty() || !text.is_char_boundary(span.start) {
        return None;
    }
    let byte = *text.as_bytes().get(span.start)?;
    if !(0x20..=0x7e).contains(&byte) || matches!(byte, b'\'' | b'"' | b'\\') {
        return None;
    }
    let line = text[..span.start].split('\n').count();
    Some(format!(
        "line {line}: expected '.' or '=', but got '{}' instead",
        char::from(byte)
    ))
}

/// Healthy configuration helper used by the native fixture tests.
#[cfg(test)]
pub(super) fn parse_and_validate(doc: &DocumentMut) -> Option<LoadedConfig> {
    decode_config(doc.as_table())?.validate()?.ok()
}
