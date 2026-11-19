//! Top-level commands that did not exist in the Go oracle (`migration/cli-additions.md`).
//!
//! The frozen `USAGE` text in `lib.rs` is the Go recording and never changes.
//! New commands are declared in `assets/cli-additions.json`; the usage the
//! binary prints is that recording with one line per declaration inserted after
//! the last line of the first command group.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

use serde::Deserialize;

/// Last recorded line of the first command group; additions follow it.
const ANCHOR: &str =
    "  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)\n";
/// Width of the name column in the recorded list (`  init        ...`).
const NAME_WIDTH: usize = 11;
const MACOS_NOTE: &str = " (macOS only)";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Platform {
    Any,
    Macos,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Addition {
    pub(crate) name: String,
    pub(crate) short: String,
    pub(crate) platform: Platform,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema_version: u32,
    additions: Vec<Addition>,
}

fn parse(json: &str) -> Result<Vec<Addition>, String> {
    let file: File = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if file.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}",
            file.schema_version
        ));
    }
    Ok(file.additions)
}

/// The additions this product declares.
pub(crate) fn declared() -> &'static [Addition] {
    static DECLARED: OnceLock<Vec<Addition>> = OnceLock::new();
    DECLARED.get_or_init(|| {
        parse(include_str!("../assets/cli-additions.json")).expect("assets/cli-additions.json")
    })
}

/// The top-level usage: the recorded text plus the declared additions.
pub(crate) fn usage() -> &'static str {
    static USAGE: OnceLock<String> = OnceLock::new();
    USAGE.get_or_init(|| render(crate::USAGE, declared()).expect("assets/cli-additions.json"))
}

/// Command names the recorded text already lists (`  name  description`).
fn recorded_commands(frozen: &str) -> BTreeSet<&str> {
    frozen
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|rest| !rest.starts_with(' '))
        .filter_map(|rest| rest.split(' ').next())
        .collect()
}

fn check(addition: &Addition, taken: &BTreeSet<&str>) -> Result<(), String> {
    let name = addition.name.as_str();
    let valid_name = name.len() <= NAME_WIDTH
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_name {
        return Err(format!("invalid command name {name:?}"));
    }
    if taken.contains(name) {
        return Err(format!("{name}: already a recorded or declared command"));
    }
    if addition.short.is_empty() || addition.short.chars().any(char::is_control) {
        return Err(format!("{name}: short must be one non-empty line"));
    }
    Ok(())
}

/// `frozen` with one list line per addition inserted after [`ANCHOR`].
fn render(frozen: &str, additions: &[Addition]) -> Result<String, String> {
    if frozen.matches(ANCHOR).count() != 1 {
        return Err("anchor line not found exactly once in the recorded usage".to_owned());
    }
    let mut taken = recorded_commands(frozen);
    let mut lines = String::new();
    for addition in additions {
        check(addition, &taken)?;
        taken.insert(addition.name.as_str());
        let note = if addition.platform == Platform::Macos {
            MACOS_NOTE
        } else {
            ""
        };
        let _ = writeln!(
            lines,
            "  {:<NAME_WIDTH$} {}{note}",
            addition.name, addition.short
        );
    }
    Ok(frozen.replacen(ANCHOR, &format!("{ANCHOR}{lines}"), 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_cli::WORKERS;

    fn addition(name: &str, short: &str) -> Addition {
        Addition {
            name: name.to_owned(),
            short: short.to_owned(),
            platform: Platform::Any,
        }
    }

    #[test]
    fn usage_is_the_recording_plus_exactly_the_declared_lines() {
        let printed = usage();
        let mut stripped = printed.to_owned();
        for line in printed.lines() {
            let name = line
                .strip_prefix("  ")
                .and_then(|rest| rest.split(' ').next());
            if declared().iter().any(|a| Some(a.name.as_str()) == name) {
                stripped = stripped.replacen(&format!("{line}\n"), "", 1);
            }
        }
        assert_eq!(stripped, crate::USAGE);
        assert!(printed.len() > crate::USAGE.len());
    }

    #[test]
    fn declared_lines_follow_the_last_recorded_command_in_order() {
        let tail = usage()
            .split_once(ANCHOR)
            .expect("anchor present")
            .1
            .lines()
            .take(3)
            .collect::<Vec<_>>();
        assert!(
            tail[0].starts_with("  browse      Agent-operated browser"),
            "{tail:?}"
        );
        assert!(tail[1].starts_with("  operate     "), "{tail:?}");
        assert!(tail[1].ends_with(" (macOS only)"), "{tail:?}");
        assert!(tail[2].starts_with("  scope       "), "{tail:?}");
        assert!(tail[2].ends_with(" (macOS only)"), "{tail:?}");
    }

    #[test]
    fn additions_never_shadow_a_recorded_command() {
        for name in [
            "guard", "version", "help", "init", "vault", "memory", "symbrain",
        ] {
            let error = render(crate::USAGE, &[addition(name, "x")]).expect_err(name);
            assert!(error.contains("already"), "{name}: {error}");
        }
        let twice = [addition("fresh", "a"), addition("fresh", "b")];
        assert!(render(crate::USAGE, &twice).is_err());
    }

    #[test]
    fn malformed_entries_are_rejected() {
        for (name, short) in [
            ("", "x"),
            ("Upper", "x"),
            ("with space", "x"),
            ("-flag", "x"),
            ("twelve-chars", "x"),
            ("ok", ""),
            ("ok", "two\nlines"),
        ] {
            assert!(
                render(crate::USAGE, &[addition(name, short)]).is_err(),
                "{name:?} {short:?}"
            );
        }
        assert!(render("no anchor here\n", &[]).is_err());
        assert!(parse(r#"{"schema_version":2,"additions":[]}"#).is_err());
        assert!(parse(r#"{"schema_version":1,"additions":[],"x":1}"#).is_err());
        let unknown_platform =
            r#"{"schema_version":1,"additions":[{"name":"a","short":"b","platform":"beos"}]}"#;
        assert!(parse(unknown_platform).is_err());
    }

    #[test]
    fn every_worker_command_is_declared_with_its_platform() {
        for worker in &WORKERS {
            let found = declared()
                .iter()
                .find(|a| a.name == worker.command)
                .unwrap_or_else(|| panic!("{} is dispatched but not declared", worker.command));
            assert_eq!(
                found.platform == Platform::Macos,
                worker.macos_only,
                "{}",
                worker.command
            );
        }
    }
}
