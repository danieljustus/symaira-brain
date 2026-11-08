//! Globally disabled skill targets (`disabled_targets` in the symskills config).
//!
//! A harness that was removed from the machine (for example `openclaw`) stays
//! a registered target, so without a switch it is rendered, scanned and counted
//! by validator hints forever. The top-level `disabled_targets = ["openclaw"]`
//! array of `config.toml` and `.symskills.toml` turns such targets off for
//! rendering, installing, status scans and validator hints; `targets` lists
//! them as `disabled`. This is separate from the per-skill `disabled` flag in
//! `symskills.toml` and from the `[[targets]]` custom-target tables.
//!
//! Reading is deliberately tolerant: a missing or unparsable file, a value that
//! is not an array and non-string entries contribute nothing. The typed config
//! decode reports real config errors; this switch must never be the reason a
//! command fails.

use std::path::Path;

use toml_edit::DocumentMut;

/// The config key that lists disabled target names.
pub const KEY: &str = "disabled_targets";

/// Disabled target names declared by one config document.
#[must_use]
pub fn parse(text: &str) -> Vec<String> {
    let Ok(document) = text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let Some(array) = document.get(KEY).and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Union of the disabled targets of several config files, in first-seen order.
#[must_use]
pub fn load(paths: &[&Path]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for name in parse(&text) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Whether `target` is listed in `disabled`.
#[must_use]
pub fn contains(disabled: &[String], target: &str) -> bool {
    disabled.iter().any(|name| name == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_only_a_top_level_string_array() {
        let cases: [(&str, &[&str]); 9] = [
            ("", &[]),
            ("disabled_targets = [\"openclaw\"]\n", &["openclaw"]),
            (
                "disabled_targets = [\" openclaw \", \"\", 3, \"codex\"]\n",
                &["openclaw", "codex"],
            ),
            ("disabled_targets = \"openclaw\"\n", &[]),
            ("[targets]\ndisabled = [\"openclaw\"]\n", &[]),
            ("[x]\ndisabled_targets = [\"openclaw\"]\n", &[]),
            ("disabled_targets = [\n", &[]),
            ("DISABLED_TARGETS = [\"openclaw\"]\n", &[]),
            ("disabled_targets = []\n", &[]),
        ];
        for (text, expected) in cases {
            assert_eq!(parse(text), expected, "{text:?}");
        }
    }

    #[test]
    fn load_unions_files_and_skips_unreadable_ones() {
        let dir = tempfile::tempdir().expect("temp");
        let global = dir.path().join("config.toml");
        let project = dir.path().join(".symskills.toml");
        let missing = dir.path().join("missing.toml");
        std::fs::write(&global, "disabled_targets = [\"openclaw\", \"hermes\"]\n").unwrap();
        std::fs::write(&project, "disabled_targets = [\"hermes\", \"codex\"]\n").unwrap();
        assert_eq!(
            load(&[&global, &missing, &project]),
            ["openclaw", "hermes", "codex"]
        );
        assert!(contains(&load(&[&global]), "openclaw"));
        assert!(!contains(&load(&[&global]), "codex"));
    }
}
