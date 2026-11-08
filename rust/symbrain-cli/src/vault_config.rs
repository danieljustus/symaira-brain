//! Preserve Go's config-load failure boundary before binary overrides.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

pub(super) fn configured_override(path: &Path, variable: &str) -> Option<PathBuf> {
    let global = read_document(path)?;
    let project = match std::env::current_dir() {
        Ok(directory) => read_document(&directory.join(".symbrain.toml"))?,
        Err(_) => DocumentMut::new(),
    };
    resolve(&global, &project, variable, |name| std::env::var_os(name))
}

/// Applies `CoreKit` v0.18.2 `configkit` semantics: every present value is
/// converted (zero values are not skipped), a layer that sets `binary_path`
/// replaces the earlier one even when empty, and the environment is read like
/// `os.LookupEnv`, so a present-but-empty variable counts as set.
fn resolve(
    global: &DocumentMut,
    project: &DocumentMut,
    variable: &str,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    if !valid_file_types(global) || !valid_file_types(project) || !valid_environment_types(&env) {
        return None;
    }
    let value = match env(variable) {
        Some(value) => value,
        None => [project, global]
            .into_iter()
            .find_map(binary_path)
            .map(OsString::from)?,
    };
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn read_document(path: &Path) -> Option<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(text) => text.parse().ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(DocumentMut::new()),
        Err(_) => None,
    }
}

fn binary_path(document: &DocumentMut) -> Option<&str> {
    lookup(document, "servers.vault.binary_path")?.as_str()
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

fn valid_integer(value: &str) -> bool {
    // Go ParseInt with base 10 accepts one optional sign and decimal digits;
    // separators, surrounding whitespace, and values outside int64 fail.
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i64>().is_ok()
}

const TEXT_LEAVES: [&str; 4] = [
    "default_profile",
    "servers.vault.binary_path",
    "servers.operate.binary_path",
    "servers.scope.binary_path",
];

const BOOL_LEAVES: [&str; 8] = [
    "audit.enabled",
    "audit.verbose",
    "gateway.identity_injection",
    "updatecheck.enabled",
    "patterns.enabled",
    "modules.browse",
    "modules.operate",
    "modules.scope",
];

const INT_LEAF: &str = "patterns.promotion_threshold";

fn valid_bool(value: &str) -> bool {
    crate::setup_cli::parse_go_bool(value).is_ok()
}

fn valid_file_types(document: &DocumentMut) -> bool {
    // configkit converts every present value; no zero value is skipped.
    TEXT_LEAVES
        .iter()
        .all(|path| lookup(document, path).is_none_or(|item| item.as_str().is_some()))
        && BOOL_LEAVES.iter().all(|path| {
            lookup(document, path).is_none_or(|item| {
                item.as_bool().is_some() || item.as_str().is_some_and(valid_bool)
            })
        })
        && lookup(document, INT_LEAF).is_none_or(|item| {
            item.as_integer().is_some()
                || item.as_float().is_some()
                || item.as_str().is_some_and(valid_integer)
        })
}

fn valid_environment_types(env: &impl Fn(&str) -> Option<OsString>) -> bool {
    // `os.LookupEnv`: a present but empty value is converted and rejected for
    // bool and int fields instead of being ignored.
    let bools = BOOL_LEAVES.iter().all(|path| {
        env(&environment_name(path)).is_none_or(|value| value.to_str().is_some_and(valid_bool))
    });
    bools
        && env(&environment_name(INT_LEAF))
            .is_none_or(|value| value.to_str().is_some_and(valid_integer))
}

fn environment_name(path: &str) -> String {
    format!("SYMBRAIN_{}", path.replace('.', "_").to_uppercase())
}

#[cfg(test)]
#[path = "vault_config_tests.rs"]
mod tests;
