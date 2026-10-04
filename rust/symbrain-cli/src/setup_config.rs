//! Go-compatible optional-core selection for release setup.
use std::collections::BTreeMap;

pub(crate) fn enabled_cores() -> Result<BTreeMap<String, bool>, String> {
    let mut enabled = BTreeMap::new();
    let global = symbrain_core::xdg::config_path();
    merge_enabled_cores_file(&mut enabled, &global)?;

    // configkit's TOML merge applies only non-zero scalar values. Therefore an
    // explicit project `browse = false` cannot clear a prior global `true`;
    // retain that observed Go behavior here until the coordinated Go fix.
    let project = std::env::current_dir()
        .map_err(|error| format!("config: current directory: {error}"))?
        .join(".symbrain.toml");
    merge_enabled_cores_file(&mut enabled, &project)?;

    // Environment values are presence-based overrides. As in configkit, an
    // empty value is ignored, while Go's strconv.ParseBool accepts all of the
    // listed spellings below (not just true/false).
    for (module, core) in [
        ("BROWSE", "symbrowse"),
        ("OPERATE", "symoperate"),
        ("SCOPE", "symscope"),
    ] {
        let variable = format!("SYMBRAIN_MODULES_{module}");
        if let Some(value) = std::env::var_os(&variable) {
            let value = value.to_string_lossy();
            if !value.is_empty() {
                enabled.insert(
                    core.into(),
                    parse_go_bool(&value)
                        .map_err(|()| format!("config: invalid boolean {variable}={value:?}"))?,
                );
            }
        }
    }
    Ok(enabled)
}

pub(super) fn merge_enabled_cores_file(
    enabled: &mut BTreeMap<String, bool>,
    path: &std::path::Path,
) -> Result<(), String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("config: read {}: {error}", path.display())),
    };
    let values = parse_enabled_cores(&bytes, path)?;
    // Match configkit's non-zero TOML merge: false is never assigned.
    for (name, value) in values {
        if value {
            enabled.insert(name, true);
        }
    }
    Ok(())
}

pub(super) fn parse_enabled_cores(
    bytes: &[u8],
    path: &std::path::Path,
) -> Result<BTreeMap<String, bool>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("config: parse {}: {error}", path.display()))?;
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| format!("config: parse {}: {error}", path.display()))?;
    let mut enabled = BTreeMap::new();
    for (module, core) in [
        ("browse", "symbrowse"),
        ("operate", "symoperate"),
        ("scope", "symscope"),
    ] {
        if let Some(value) = document.get("modules").and_then(|item| item.get(module)) {
            let browse = value
                .as_bool()
                .or_else(|| value.as_str().and_then(|value| parse_go_bool(value).ok()))
                .or_else(|| {
                    (value.as_str() == Some("")
                        || value.as_integer() == Some(0)
                        || value.as_float() == Some(0.0))
                    .then_some(false)
                })
                .ok_or_else(|| {
                    format!(
                        "config: modules.{module} must be boolean in {}",
                        path.display()
                    )
                })?;
            enabled.insert(core.to_string(), browse);
        }
    }
    Ok(enabled)
}

pub(crate) fn parse_go_bool(value: &str) -> Result<bool, ()> {
    match value {
        "1" | "t" | "T" | "TRUE" | "True" | "true" => Ok(true),
        "0" | "f" | "F" | "FALSE" | "False" | "false" => Ok(false),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_browse_selection_matches_go_default_and_explicit_values() {
        let path = std::path::Path::new("config.toml");
        let empty = parse_enabled_cores(b"", path).unwrap();
        assert!(!empty.get("symbrowse").copied().unwrap_or(false));
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = false\n", path)
                .unwrap()
                .get("symbrowse")
                .is_some_and(|value| !value)
        );
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = true\n", path)
                .unwrap()
                .get("symbrowse")
                .is_some_and(|value| *value)
        );
        assert!(parse_enabled_cores(b"[modules]\nbrowse = 1\n", path).is_err());
    }

    #[test]
    fn go_parse_bool_spellings_and_empty_env_contract() {
        for value in ["1", "t", "T", "TRUE", "True", "true"] {
            assert_eq!(parse_go_bool(value), Ok(true), "{value}");
        }
        for value in ["0", "f", "F", "FALSE", "False", "false"] {
            assert_eq!(parse_go_bool(value), Ok(false), "{value}");
        }
        for value in ["", "yes", "on", "2"] {
            assert_eq!(parse_go_bool(value), Err(()), "{value}");
        }
    }

    #[test]
    fn toml_false_does_not_clear_prior_true_like_configkit() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.toml");
        let project = root.path().join("project.toml");
        std::fs::write(&global, b"[modules]\nbrowse = true\n").unwrap();
        std::fs::write(&project, b"[modules]\nbrowse = false\n").unwrap();
        let mut enabled = BTreeMap::new();
        merge_enabled_cores_file(&mut enabled, &global).unwrap();
        merge_enabled_cores_file(&mut enabled, &project).unwrap();
        assert_eq!(enabled.get("symbrowse"), Some(&true));
    }
}
