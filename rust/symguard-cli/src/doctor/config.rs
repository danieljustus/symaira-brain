//! Guard doctor configuration parsing and compatibility gate.
use std::io;
use std::path::{Path, PathBuf};
use std::{env, fs};
use toml_edit::{DocumentMut, Item, Table, Value};

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
}

/// Ports `config.Load()`. `None` means Go would print an error string this
/// port cannot reproduce — fall back.
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
    parse_and_validate(&doc).map(ConfigState::Loaded)
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

const VALID_DECISIONS: [&str; 6] = ["allow", "ask", "deny", "redact", "readonly", "sandbox"];

/// Decodes and validates the TOML exactly as far as doctor's output depends
/// on it. Every shape mismatch (a Go decode error) and every `validate()`
/// rejection returns `None`, because both print library or format text this
/// port does not reproduce.
pub(super) fn parse_and_validate(doc: &DocumentMut) -> Option<LoadedConfig> {
    check_defaults(doc)?;
    let rules = count_rules(doc)?;
    check_sequence(doc)?;
    check_unprinted_sections(doc)?;
    let allowlist = read_allowlist(doc)?;
    Some(LoadedConfig { rules, allowlist })
}

pub(super) fn string_of(item: &Item) -> Option<&str> {
    item.as_str()
}

pub(super) fn string_array(item: &Item) -> Option<Vec<String>> {
    item.as_array()?
        .iter()
        .map(|value| match value {
            Value::String(text) => Some(text.value().clone()),
            _ => None,
        })
        .collect()
}

/// `[defaults]` decodes to `map[string]Decision`; `validate()` rejects any
/// value that is not a known decision.
pub(super) fn check_defaults(doc: &DocumentMut) -> Option<()> {
    let Some(item) = doc.get("defaults") else {
        return Some(());
    };
    let table = item.as_table()?;
    for (_, value) in table {
        if !VALID_DECISIONS.contains(&string_of(value)?) {
            return None;
        }
    }
    Some(())
}

/// `[[rules]]`: every rule needs a valid decision and at least one match
/// criterion, else `validate()` rejects the file.
pub(super) fn count_rules(doc: &DocumentMut) -> Option<usize> {
    let Some(item) = doc.get("rules") else {
        return Some(0);
    };
    let tables = item.as_array_of_tables()?;
    for rule in tables {
        if !VALID_DECISIONS.contains(&string_of(rule.get("decision")?)?) {
            return None;
        }
        let matcher = rule.get("match")?.as_table()?;
        let mut criteria = 0usize;
        for field in ["server", "tool", "capability"] {
            if let Some(value) = matcher.get(field)
                && !string_of(value)?.is_empty()
            {
                criteria += 1;
            }
        }
        if let Some(value) = matcher.get("command_contains")
            && !string_array(value)?.is_empty()
        {
            criteria += 1;
        }
        if criteria == 0 {
            return None;
        }
    }
    Some(tables.len())
}

/// `[sequence]`: `DefaultConfig()` seeds `Threshold = 3`, and a TOML decode
/// only overwrites keys the file actually contains.
pub(super) fn check_sequence(doc: &DocumentMut) -> Option<()> {
    let Some(item) = doc.get("sequence") else {
        return Some(());
    };
    let table = item.as_table()?;
    let enabled = match table.get("enabled") {
        None => false,
        Some(value) => value.as_bool()?,
    };
    let threshold = match table.get("threshold") {
        None => 3,
        Some(value) => value.as_integer()?,
    };
    if enabled && threshold < 2 {
        return None;
    }
    Some(())
}

/// `[proxy]`, `[audit]` and `[[remote]]` never reach doctor's output, but a
/// wrong type in any of them is a Go decode error, so their shape still
/// decides native versus Go.
pub(super) fn check_unprinted_sections(doc: &DocumentMut) -> Option<()> {
    fn strings(table: &Table, fields: &[&str]) -> Option<()> {
        for field in fields {
            if let Some(value) = table.get(field) {
                string_of(value)?;
            }
        }
        Some(())
    }
    if let Some(item) = doc.get("proxy") {
        strings(item.as_table()?, &["upstream"])?;
    }
    if let Some(item) = doc.get("audit") {
        let table = item.as_table()?;
        strings(table, &["path", "encrypt_age"])?;
        if let Some(value) = table.get("encrypt") {
            value.as_bool()?;
        }
    }
    if let Some(item) = doc.get("remote") {
        for target in item.as_array_of_tables()? {
            strings(target, &["name", "provider", "host", "trust_level"])?;
            for field in ["allowed_servers", "labels"] {
                if let Some(value) = target.get(field) {
                    string_array(value)?;
                }
            }
        }
    }
    Some(())
}

/// `[[spawn.allowlist]]`: `validate()` requires a non-empty absolute path.
pub(super) fn read_allowlist(doc: &DocumentMut) -> Option<Vec<SpawnEntry>> {
    let mut allowlist = Vec::new();
    let Some(item) = doc.get("spawn") else {
        return Some(allowlist);
    };
    let Some(entries) = item.as_table()?.get("allowlist") else {
        return Some(allowlist);
    };
    for entry in entries.as_array_of_tables()? {
        let path = string_of(entry.get("path")?)?.to_owned();
        if path.is_empty() || !Path::new(&path).is_absolute() {
            return None;
        }
        let argv_prefix = match entry.get("argv_prefix") {
            None => Vec::new(),
            Some(value) => string_array(value)?,
        };
        allowlist.push(SpawnEntry { path, argv_prefix });
    }
    Some(allowlist)
}

// ---------------------------------------------------------------------------
