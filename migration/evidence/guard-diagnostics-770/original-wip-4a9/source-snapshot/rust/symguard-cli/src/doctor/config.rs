//! Guard doctor configuration parsing and compatibility gate.
use std::io;
use std::path::{Path, PathBuf};
use std::{env, fs};
use toml_edit::DocumentMut;

#[path = "config_decode.rs"]
mod decode;
use decode::decode_config;

/// Go configuration path precedence: explicit override, XDG, then home.
pub(super) fn config_path() -> PathBuf {
    if let Some(env) = env::var_os("SYMGUARD_CONFIG").filter(|v| !v.is_empty()) {
        return PathBuf::from(env);
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("symguard").join("config.toml");
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("symguard").join("config.toml")
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

/// Ports proven `config.Load()` states. `None` means a decoder diagnostic,
/// warning or map-order-dependent failure remains Go-owned.
pub(super) fn load_config(path: &Path) -> Option<ConfigState> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(ConfigState::Missing),
        Err(_) => return None,
    };
    let doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            let diagnostic = go_missing_equals_diagnostic(&text, &error)?;
            return Some(ConfigState::ParseError(format!("toml: {diagnostic}")));
        }
    };
    let decoded = decode_config(&doc)?;
    match decoded.validate()? {
        Ok(loaded) => Some(ConfigState::Loaded(loaded)),
        Err(error) => Some(ConfigState::ValidationError(error)),
    }
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
    decode_config(doc)?.validate()?.ok()
}
