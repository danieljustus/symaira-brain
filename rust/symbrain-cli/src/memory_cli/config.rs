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
    pub ollama_url: String,
    pub ollama_model: String,
    pub prefilter: bool,
    pub conflict_enabled: bool,
    pub conflict_provider: String,
    pub conflict_contradiction: Option<f64>,
    pub conflict_near_dup: Option<f64>,
    pub conflict_max_candidates: Option<i64>,
    pub quantize_binary: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: OsString::new(),
            ollama_url: "http://localhost:11434/api/embeddings".to_owned(),
            ollama_model: "nomic-embed-text".to_owned(),
            prefilter: false,
            conflict_enabled: true,
            conflict_provider: String::new(),
            conflict_contradiction: Some(0.80),
            conflict_near_dup: Some(0.95),
            conflict_max_candidates: Some(10),
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
            } else {
                apply(&mut config, path, &text);
            }
        }
    }
    Some(config)
}

fn merge(config: &mut Config, path: &std::path::Path) -> Option<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(()),
        Err(_) => return None,
    };
    let document = content.parse::<toml_edit::DocumentMut>().ok()?;
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
        } else if let Some(value) = item.as_integer() {
            apply(config, path, &value.to_string());
        } else if let Some(value) = item.as_float() {
            if path == "conflict.max_candidates" {
                // Go converts TOML floats to int. Leave this representation
                // delegated until its platform-dependent range is exercised.
                config.conflict_max_candidates = None;
            } else {
                apply(config, path, &value.to_string());
            }
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
        "ollama.url" => value.clone_into(&mut config.ollama_url),
        "ollama.model" => value.clone_into(&mut config.ollama_model),
        "hybrid_search.prefilter_enabled" => {
            config.prefilter = config_value::parse_bool(value).unwrap_or(false);
        }
        "conflict.enabled" => {
            config.conflict_enabled = config_value::parse_bool(value).unwrap_or(true);
        }
        "conflict.llm_provider" => value.clone_into(&mut config.conflict_provider),
        "conflict.contradiction_threshold" => {
            config.conflict_contradiction = conflict_float(value);
        }
        "conflict.near_dup_threshold" => config.conflict_near_dup = conflict_float(value),
        "conflict.max_candidates" => config.conflict_max_candidates = value.parse().ok(),
        "hybrid_search.quantize_to_binary" => {
            config.quantize_binary = config_value::parse_bool(value).unwrap_or(false);
        }
        _ => {}
    }
}

fn conflict_float(value: &str) -> Option<f64> {
    // Validation remains configkit-compatible. Valid but unsupported hex and
    // nonfinite representations retain Go, never silently select defaults.
    value.replace('_', "").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Config {
    pub(super) fn deterministic_conflict_supported(&self, staged: bool) -> bool {
        self.conflict_provider.is_empty()
            && (staged || !self.conflict_enabled || (!self.prefilter
                && self.conflict_contradiction.is_some()
                && self.conflict_near_dup.is_some()
                && self.conflict_max_candidates.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_admission_preserves_unsupported_configurations() {
        let mut config = Config::default();
        assert!(config.deterministic_conflict_supported(false));
        for provider in ["ollama", "openai", " "] {
            apply(&mut config, "conflict.llm_provider", provider);
            assert!(!config.deterministic_conflict_supported(false));
            assert!(!config.deterministic_conflict_supported(true));
        }
        config.conflict_provider.clear();
        for text in ["0x1.ep-1", "NaN", "+Inf"] {
            apply(&mut config, "conflict.near_dup_threshold", text);
            assert!(!config.deterministic_conflict_supported(false));
            assert!(config.deterministic_conflict_supported(true));
        }
        apply(&mut config, "conflict.near_dup_threshold", "0.9_5");
        assert_eq!(config.conflict_near_dup, Some(0.95));
        config.prefilter = true;
        assert!(!config.deterministic_conflict_supported(false));
        config.conflict_enabled = false;
        assert!(config.deterministic_conflict_supported(false));
    }

    #[test]
    fn numeric_file_and_string_overrides_keep_configkit_zero_semantics() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("config.toml");
        let mut config = Config::default();
        std::fs::write(&file, "[conflict]\nnear_dup_threshold=0.91\ncontradiction_threshold=0.7\nmax_candidates=3\n").unwrap();
        merge(&mut config, &file).unwrap();
        assert_eq!(config.conflict_near_dup, Some(0.91));
        assert_eq!(config.conflict_contradiction, Some(0.7));
        assert_eq!(config.conflict_max_candidates, Some(3));
        std::fs::write(&file, "[conflict]\nenabled=false\nnear_dup_threshold=0\nmax_candidates=0\n").unwrap();
        merge(&mut config, &file).unwrap();
        assert!(config.conflict_enabled);
        assert_eq!(config.conflict_near_dup, Some(0.91));
        assert_eq!(config.conflict_max_candidates, Some(3));
        // Environment/string values are not zero-skipped; checker normalizes.
        apply(&mut config, "conflict.near_dup_threshold", "0");
        apply(&mut config, "conflict.max_candidates", "-1");
        assert_eq!(config.conflict_near_dup, Some(0.0));
        assert_eq!(config.conflict_max_candidates, Some(-1));
        std::fs::write(&file, "[conflict]\nmax_candidates=1.5\n").unwrap();
        merge(&mut config, &file).unwrap();
        assert!(!config.deterministic_conflict_supported(false));
        apply(&mut config, "conflict.max_candidates", "2");
        assert!(config.deterministic_conflict_supported(false));
    }
}
