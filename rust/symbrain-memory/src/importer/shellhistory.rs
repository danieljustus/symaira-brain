//! Shell-history importer (ported for #761 from
//! `importer/shellhistory/shell_history.go` at v0.12.0).
//!
//! Reads a zsh (extended format) or bash (`#epoch` markers) history file that
//! the caller names; with no path the Go default is detected below `$HOME`.
//! Each timestamped command that survives the exclusion list and filters
//! becomes one session whose fact is `User ran: <cmd>`.

use super::util::{base_name, home_dir, scan_lines};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::pii::go_regex;
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

const EXCLUDED: [&str; 9] = [
    "cd", "ls", "pwd", "echo", "cat", "exit", "logout", "history", "clear",
];

const TAGS: [(&str, &str); 13] = [
    ("brew", "package-manager"),
    ("npm", "package-manager"),
    ("yarn", "package-manager"),
    ("pnpm", "package-manager"),
    ("pip", "package-manager"),
    ("uv", "package-manager"),
    ("go", "vcs"),
    ("git", "vcs"),
    ("gh", "github"),
    ("docker", "container"),
    ("make", "build"),
    ("cargo", "build"),
    ("cmake", "build"),
];

fn zsh_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| go_regex(r"^: (\d+):\d+;(.+)"))
}

fn bash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| go_regex(r"^#(\d+)$"))
}

/// Imports shell command history.
pub struct ShellHistory {
    history_path: String,
    filters: Vec<String>,
}

impl ShellHistory {
    /// `history_path` empty detects the default; `filters` keep only commands
    /// containing one of them. (`successOnly` was accepted but unused in Go.)
    #[must_use]
    pub fn new(history_path: &str, filters: Vec<String>) -> Self {
        let history_path = if history_path.is_empty() {
            detect_history_path()
        } else {
            history_path.to_owned()
        };
        Self {
            history_path,
            filters,
        }
    }

    fn matches_filter(&self, command: &str) -> bool {
        self.filters.is_empty() || self.filters.iter().any(|f| command.contains(f.as_str()))
    }
}

/// `detectHistoryPath`: `~/.zsh_history` for zsh when it exists, else
/// `~/.bash_history`; empty when `$HOME` is unknown.
fn detect_history_path() -> String {
    let Ok(home) = home_dir() else {
        return String::new();
    };
    if std::env::var("SHELL").is_ok_and(|s| s.contains("zsh")) && home.join(".zsh_history").exists()
    {
        return home.join(".zsh_history").to_string_lossy().into_owned();
    }
    home.join(".bash_history").to_string_lossy().into_owned()
}

/// `parseHistoryLine`: `(epoch seconds, command)`; `None` epoch for lines
/// without a timestamp.
fn parse_history_line(line: &str) -> (Option<i64>, String) {
    if let Some(m) = zsh_re().captures(line) {
        return (m[1].parse().ok(), m[2].to_owned());
    }
    if let Some(m) = bash_re().captures(line) {
        return (m[1].parse().ok(), String::new());
    }
    (None, line.to_owned())
}

fn tag_command(command: &str) -> Option<&'static str> {
    let first = command.split_whitespace().next()?;
    let bin = base_name(Path::new(first));
    TAGS.iter()
        .find(|(name, _)| *name == bin)
        .map(|(_, tag)| *tag)
}

impl SessionImporter for ShellHistory {
    fn name(&self) -> &'static str {
        "shell-history"
    }
    fn category(&self) -> Option<&str> {
        Some("code")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        if self.history_path.is_empty() {
            return Err("no shell history file found".into());
        }
        let (lines, scan_error) = scan_lines(Path::new(&self.history_path), 64 * 1024)
            .map_err(|e| format!("failed to open history file: {e}"))?;
        let mut sessions = Vec::new();
        for line in lines.iter().filter(|l| !l.is_empty()) {
            let (epoch, command) = parse_history_line(line);
            let Some(ts) = epoch.and_then(|e| DateTime::from_timestamp(e, 0)) else {
                continue;
            };
            if ts < since || EXCLUDED.contains(&command.as_str()) {
                continue;
            }
            if command
                .split_whitespace()
                .next()
                .is_some_and(|w| EXCLUDED.contains(&w))
            {
                continue;
            }
            if !self.matches_filter(&command) {
                continue;
            }
            sessions.push(SessionRef {
                tool: "shell-history".into(),
                session_id: ts.timestamp().to_string(),
                path: self.history_path.clone(),
                modified_at: ts,
                metadata: BTreeMap::from([("command".to_owned(), command)]),
            });
        }
        scan_error.map_or(Ok(sessions), Err)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let command = session.metadata.get("command").cloned().unwrap_or_default();
        if command.is_empty() {
            return Ok(Vec::new());
        }
        let mut metadata = BTreeMap::from([
            ("command".to_owned(), command.clone()),
            ("source".to_owned(), "shell-history".to_owned()),
        ]);
        if let Some(tag) = tag_command(&command) {
            metadata.insert("tag".into(), tag.into());
        }
        Ok(vec![ImportedFact {
            content: format!("User ran: {command}"),
            source: "shell-history".into(),
            session_id: session.session_id.clone(),
            timestamp: session.modified_at,
            metadata,
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::test_support::TestDir;
    use chrono::TimeZone;

    fn at(day: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap() + chrono::Duration::days(day)
    }

    #[test]
    fn parses_zsh_and_bash_lines() {
        assert_eq!(
            parse_history_line(": 1700000000:0;git status"),
            (Some(1_700_000_000), "git status".into())
        );
        assert_eq!(
            parse_history_line("#1700000001"),
            (Some(1_700_000_001), String::new())
        );
        assert_eq!(
            parse_history_line("plain command"),
            (None, "plain command".into())
        );
        assert_eq!(
            parse_history_line(": 12:0;"),
            (None, ": 12:0;".into()),
            "empty command does not match zsh form"
        );
    }

    #[test]
    fn discovers_filters_and_tags() {
        let dir = TestDir::new("shell");
        let (a, b, c, d) = (
            at(1).timestamp(),
            at(2).timestamp(),
            at(3).timestamp(),
            at(4).timestamp(),
        );
        let history = dir.write(
            "history",
            &format!(
                ": {a}:0;cargo test -p x\n: {b}:3;ls -la\n: {c}:0;cd /tmp\n: {d}:0;/usr/bin/git log\nno-timestamp\n#{a}\n\n: 1:0;too old\n"
            ),
        );
        let path = history.to_string_lossy().into_owned();
        let importer = ShellHistory::new(&path, Vec::new());
        let sessions = importer.discover_sessions(at(0)).unwrap();
        let commands: Vec<_> = sessions
            .iter()
            .map(|s| s.metadata["command"].as_str())
            .collect();
        // `#a` (bash marker) yields a session with an empty command; excluded
        // commands, untimestamped and too-old lines are dropped.
        assert_eq!(commands, ["cargo test -p x", "/usr/bin/git log", ""]);
        assert_eq!(sessions[0].session_id, a.to_string());
        assert_eq!(sessions[0].modified_at, at(1));
        let fact = &importer.import_session(&sessions[0]).unwrap()[0];
        assert_eq!(fact.content, "User ran: cargo test -p x");
        assert_eq!(fact.metadata["tag"], "build");
        assert_eq!(fact.metadata["source"], "shell-history");
        assert_eq!(
            importer.import_session(&sessions[1]).unwrap()[0].metadata["tag"],
            "vcs"
        );
        assert!(importer.import_session(&sessions[2]).unwrap().is_empty());
        let filtered = ShellHistory::new(&path, vec!["git".into()]);
        assert_eq!(filtered.discover_sessions(at(0)).unwrap().len(), 1);
        assert_eq!(importer.category(), Some("code"));
        assert_eq!(importer.privacy_level(), Some(PrivacyLevel::Confidential));
    }

    #[test]
    fn missing_file_and_empty_path_errors() {
        let dir = TestDir::new("shell-missing");
        let missing = dir.path().join("nope").to_string_lossy().into_owned();
        let error = ShellHistory::new(&missing, Vec::new())
            .discover_sessions(at(0))
            .unwrap_err();
        assert!(
            error.starts_with("failed to open history file: "),
            "{error}"
        );
        let empty = ShellHistory {
            history_path: String::new(),
            filters: Vec::new(),
        };
        assert_eq!(
            empty.discover_sessions(at(0)).unwrap_err(),
            "no shell history file found"
        );
    }
}
