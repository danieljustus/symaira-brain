//! Preserve Go's config-load failure boundary before binary overrides.
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

pub(crate) fn valid_configuration() -> bool {
    let Some(global) = read_document(&symbrain_core::xdg::config_path()) else {
        return false;
    };
    let Ok(directory) = std::env::current_dir() else {
        return false;
    };
    let Some(project) = read_document(&directory.join(".symbrain.toml")) else {
        return false;
    };
    valid_file_types(&global) && valid_file_types(&project) && valid_environment_types()
}

pub(super) fn configured_override(path: &Path, variable: &str) -> Option<PathBuf> {
    let global = read_document(path)?;
    let project = match std::env::current_dir() {
        Ok(directory) => read_document(&directory.join(".symbrain.toml"))?,
        Err(_) => DocumentMut::new(),
    };
    if !valid_file_types(&global) || !valid_file_types(&project) || !valid_environment_types() {
        return None;
    }
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| binary_path(&project).or_else(|| binary_path(&global)))
}

fn read_document(path: &Path) -> Option<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(text) => text.parse().ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(DocumentMut::new()),
        Err(_) => None,
    }
}

fn binary_path(document: &DocumentMut) -> Option<PathBuf> {
    let value = lookup(document, "servers.vault.binary_path")?.as_str()?;
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn lookup<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut table: &dyn toml_edit::TableLike = document.as_table();
    let mut parts = path.split('.').peekable();
    while let Some(part) = parts.next() {
        let item = table.get(part)?;
        if parts.peek().is_none() {
            return Some(item);
        }
        // Go ignores scalar/array values for struct fields.
        table = item.as_table_like()?;
    }
    None
}

fn zero(item: &Item) -> bool {
    item.as_str() == Some("")
        || item.as_integer() == Some(0)
        || item.as_float() == Some(0.0)
        || item.as_bool() == Some(false)
}

fn valid_bool(value: &str) -> bool {
    matches!(
        value,
        "1" | "t" | "T" | "true" | "TRUE" | "True" | "0" | "f" | "F" | "false" | "FALSE" | "False"
    )
}

fn valid_integer(value: &str) -> bool {
    // Go ParseInt with base 10 accepts one optional sign and decimal digits;
    // separators, surrounding whitespace, and values outside int64 fail.
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i64>().is_ok()
}

fn valid_file_types(document: &DocumentMut) -> bool {
    // CoreKit applies nonzero values to plain fields and every present value
    // to pointer fields. Bool/int strings use the same conversion as env.
    for path in [
        "default_profile",
        "servers.vault.binary_path",
        "servers.operate.binary_path",
        "servers.scope.binary_path",
    ] {
        if let Some(item) = lookup(document, path)
            && !zero(item)
            && item.as_str().is_none()
        {
            return false;
        }
    }
    for (path, pointer) in [
        ("audit.enabled", true),
        ("audit.verbose", false),
        ("gateway.identity_injection", true),
        ("updatecheck.enabled", true),
        ("patterns.enabled", true),
        ("modules.browse", false),
        ("modules.operate", false),
        ("modules.scope", false),
    ] {
        if let Some(item) = lookup(document, path)
            && (pointer || !zero(item))
            && item.as_bool().is_none()
            && !item.as_str().is_some_and(valid_bool)
        {
            return false;
        }
    }
    lookup(document, "patterns.promotion_threshold").is_none_or(|item| {
        zero(item)
            || item.as_integer().is_some()
            || item.as_float().is_some()
            || item.as_str().is_some_and(valid_integer)
    })
}

fn valid_environment_types() -> bool {
    for variable in [
        "SYMBRAIN_AUDIT_ENABLED",
        "SYMBRAIN_AUDIT_VERBOSE",
        "SYMBRAIN_GATEWAY_IDENTITY_INJECTION",
        "SYMBRAIN_UPDATECHECK_ENABLED",
        "SYMBRAIN_PATTERNS_ENABLED",
        "SYMBRAIN_MODULES_BROWSE",
        "SYMBRAIN_MODULES_OPERATE",
        "SYMBRAIN_MODULES_SCOPE",
    ] {
        if let Some(value) = std::env::var_os(variable)
            && !value.is_empty()
            && !value.to_str().is_some_and(valid_bool)
        {
            return false;
        }
    }
    let Some(value) = std::env::var_os("SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD") else {
        return true;
    };
    if value.is_empty() {
        return true;
    }
    value.to_str().is_some_and(valid_integer)
}
