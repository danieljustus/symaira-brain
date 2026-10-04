//! One-shot memory config: global TOML, project TOML, then typed environment.
//!
//! The shipped CLI discards the entire merged config on any loader error. In
//! particular, invalid unrelated known fields also discard a database override.

use std::ffi::OsString;
use std::path::PathBuf;

use super::config_schema::{FIELDS, Kind};
use super::config_value;

pub(super) struct Config {
    pub database: OsString,
    pub jwt_secret: OsString,
    pub jwt_secret_path: OsString,
    pub ollama_url: String,
    pub ollama_model: String,
    pub prefilter: bool,
    pub conflict_enabled: bool,
    pub quantize_binary: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: OsString::new(),
            jwt_secret: OsString::new(),
            jwt_secret_path: OsString::new(),
            ollama_url: "http://localhost:11434/api/embeddings".to_owned(),
            ollama_model: "nomic-embed-text".to_owned(),
            prefilter: false,
            conflict_enabled: true,
            quantize_binary: false,
        }
    }
}

pub(super) fn load() -> &'static Config {
    // Go configkit caches the first Load result, including errors. Routing,
    // database resolution and embedding generation must share one snapshot.
    static CONFIG: std::sync::OnceLock<Config> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| load_checked().unwrap_or_default())
}

fn load_checked() -> Option<Config> {
    // configkit checks UserHomeDir before consulting XDG or project/env values.
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let mut config = Config::default();
    merge(&mut config, &root.join("symmemory/config.toml"))?;
    if let Ok(cwd) = std::env::current_dir() {
        merge(&mut config, &cwd.join(".symmemory.toml"))?;
    }
    for &(path, kind) in FIELDS {
        // configkit explicitly skips maps for environment overrides.
        if matches!(kind, Kind::Map) {
            continue;
        }
        let name = format!("SYMMEMORY_{}", path.replace('.', "_").to_uppercase());
        if let Some(raw) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            let text = super::flags::go_string(&super::flags::bytes(&raw));
            config_value::validate_env(kind, &text)?;
            if path == "database.path" {
                config.database = raw;
            } else if path == "jwt.secret" {
                config.jwt_secret = raw;
            } else if path == "jwt.secret_path" {
                config.jwt_secret_path = raw;
            } else {
                apply(&mut config, path, &text);
            }
        }
    }
    Some(config)
}

fn merge(config: &mut Config, path: &std::path::Path) -> Option<()> {
    let content = match std::fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(()),
        Err(_) => return None,
    };
    merge_bytes(config, &content)
}

fn merge_bytes(config: &mut Config, content: &[u8]) -> Option<()> {
    let document = symbrain_core::config::resolved::parse_document(content).ok()?;
    for &(path, kind) in FIELDS {
        let mut item = document.as_item();
        for segment in path.split('.') {
            let Some(table) = item.as_table_like() else {
                // A non-table section is silently ignored by configkit.
                item = &toml_edit::Item::None;
                break;
            };
            let Some(next) = table.get(segment) else {
                item = &toml_edit::Item::None;
                break;
            };
            item = next;
        }
        if item.is_none() || (!matches!(kind, Kind::PointerBool) && is_zero(item)) {
            continue;
        }
        config_value::validate_item(kind, item)?;
        if let Some(value) = item.as_str() {
            apply(config, path, value);
        } else if let Some(value) = item.as_bool() {
            apply(config, path, if value { "true" } else { "false" });
        }
    }
    Some(())
}

fn is_zero(item: &toml_edit::Item) -> bool {
    item.as_str().is_some_and(str::is_empty)
        || item.as_integer() == Some(0)
        || item.as_float() == Some(0.0)
        || item.as_bool() == Some(false)
}

fn apply(config: &mut Config, path: &str, value: &str) {
    match path {
        "database.path" => config.database = OsString::from(value),
        "jwt.secret" => config.jwt_secret = OsString::from(value),
        "jwt.secret_path" => {
            config.jwt_secret_path = symbrain_core::go_path::from_bytes(value.as_bytes())
        }
        "ollama.url" => value.clone_into(&mut config.ollama_url),
        "ollama.model" => value.clone_into(&mut config.ollama_model),
        "hybrid_search.prefilter_enabled" => {
            config.prefilter = config_value::parse_bool(value).unwrap_or(false);
        }
        "conflict.enabled" => {
            config.conflict_enabled = config_value::parse_bool(value).unwrap_or(true);
        }
        "hybrid_search.quantize_to_binary" => {
            config.quantize_binary = config_value::parse_bool(value).unwrap_or(false);
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "config_document_tests.rs"]
mod document_tests;
