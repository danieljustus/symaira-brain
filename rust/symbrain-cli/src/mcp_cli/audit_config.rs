//! Global `audit.enabled` resolution for `symbrain mcp` startup (#834).
//!
//! Audit is on only when the selected profile *and* the resolved global
//! setting enable it, so a disabled core never creates the audit directory or
//! log. The file is validated before the environment override is applied, so
//! an opt-out cannot hide a malformed configuration.

use std::env;

use toml_edit::DocumentMut;

const AUDIT_ENABLED_ENV: &str = "SYMBRAIN_AUDIT_ENABLED";

/// Resolves the effective global `audit.enabled` (default `true`).
///
/// # Errors
/// Returns a `config: ...` diagnostic when the config file cannot be read or
/// parsed, or when `audit.enabled` / `SYMBRAIN_AUDIT_ENABLED` is not a boolean.
pub(super) fn load_global_audit_enabled() -> Result<bool, String> {
    let path = symbrain_core::xdg::config_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "config: failed to load {}: {error}",
                path.display()
            ));
        }
    };
    let configured = match contents {
        Some(contents) => configured_audit_enabled(&contents, &path.display().to_string())?,
        None => true,
    };
    resolve_with_env(configured, env::var_os(AUDIT_ENABLED_ENV))
}

fn resolve_with_env(configured: bool, value: Option<std::ffi::OsString>) -> Result<bool, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(configured);
    };
    let value = value
        .to_str()
        .ok_or_else(|| format!("config: {AUDIT_ENABLED_ENV} is not valid UTF-8"))?;
    parse_config_bool(value).ok_or_else(|| format!("config: {AUDIT_ENABLED_ENV} must be a boolean"))
}

fn configured_audit_enabled(contents: &str, path: &str) -> Result<bool, String> {
    let document = contents
        .parse::<DocumentMut>()
        .map_err(|error| format!("config: failed to parse {path}: {error}"))?;
    // A non-table top-level `audit` is ignored by the frozen loader; that is
    // distinct from an invalid `audit.enabled` value.
    let Some(table) = document.get("audit").and_then(|item| item.as_table_like()) else {
        return Ok(true);
    };
    let Some(enabled) = table.get("enabled") else {
        return Ok(true);
    };
    if let Some(value) = enabled.as_bool() {
        return Ok(value);
    }
    enabled
        .as_str()
        .and_then(parse_config_bool)
        .ok_or_else(|| "config: audit.enabled must be a boolean".to_string())
}

fn parse_config_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{configured_audit_enabled, resolve_with_env};

    #[test]
    fn configured_value_table() {
        let cases: &[(&str, Result<&bool, &str>)] = &[
            ("", Ok(&true)),
            ("[audit]\nverbose = true\n", Ok(&true)),
            ("[audit]\nenabled = false\n", Ok(&false)),
            ("[audit]\nenabled = true\n", Ok(&true)),
            ("[audit]\nenabled = \"false\"\n", Ok(&false)),
            ("[audit]\nenabled = \"T\"\n", Ok(&true)),
            ("audit = \"off\"\n", Ok(&true)),
            (
                "[audit]\nenabled = 3\n",
                Err("config: audit.enabled must be a boolean"),
            ),
            (
                "[audit]\nenabled = \"maybe\"\n",
                Err("config: audit.enabled must be a boolean"),
            ),
        ];
        for (input, expected) in cases {
            let got = configured_audit_enabled(input, "x");
            assert_eq!(got.as_ref().map_err(String::as_str), *expected, "{input:?}");
        }
        assert!(
            configured_audit_enabled("[audit\n", "x")
                .unwrap_err()
                .starts_with("config: failed to parse x")
        );
    }

    #[test]
    fn env_override_applies_after_validation() {
        let var = |value: &str| Some(std::ffi::OsString::from(value));
        assert_eq!(resolve_with_env(true, None), Ok(true));
        assert_eq!(resolve_with_env(true, var("")), Ok(true));
        assert_eq!(resolve_with_env(true, var("false")), Ok(false));
        assert_eq!(resolve_with_env(false, var("1")), Ok(true));
        assert_eq!(
            resolve_with_env(true, var("nope")),
            Err("config: SYMBRAIN_AUDIT_ENABLED must be a boolean".to_string())
        );
    }
}
