//! JSONL/Markdown transcript importers: Claude Code, Codex CLI and Aider
//! (ported for #761 from `importer/{claudecode,codex,aider}` at v0.12.0).
//!
//! Each reads assistant turns longer than 50 bytes. Locations are explicit
//! constructor arguments; an empty location falls back to the Go default
//! below `$HOME` at discovery time.

use super::util::{
    base_name, dir_name, home_dir, modified_at, parse_timestamp, scan_lines, walk_files,
};
use super::{ImportResult, ImportedFact, SessionImporter, SessionRef};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_LINE: usize = 1024 * 1024;

fn base_or_home(custom: &str, home_relative: &[&str]) -> ImportResult<PathBuf> {
    if custom.is_empty() {
        let mut path = home_dir()?;
        path.extend(home_relative);
        Ok(path)
    } else {
        Ok(PathBuf::from(custom))
    }
}

/// Imports `~/.claude/projects/**/*.jsonl`.
pub struct ClaudeCode {
    custom_path: String,
}

impl ClaudeCode {
    /// `custom_path` empty means `$HOME/.claude/projects`.
    #[must_use]
    pub fn new(custom_path: &str) -> Self {
        Self {
            custom_path: custom_path.to_owned(),
        }
    }
}

#[derive(Deserialize, Default)]
struct ClaudeMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
}

#[derive(Deserialize, Default)]
struct ClaudeEntry {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: Option<ClaudeMessage>,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    model: String,
}

impl SessionImporter for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }
    fn is_transcript(&self) -> bool {
        true
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let base = base_or_home(&self.custom_path, &[".claude", "projects"])?;
        Ok(walk_files(&base)
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(".jsonl"))
            .filter_map(|path| {
                let modified = modified_at(&path).filter(|m| *m >= since)?;
                let parent = path.parent().map(base_name).unwrap_or_default();
                Some(SessionRef {
                    tool: "claude-code".into(),
                    session_id: base_name(&path).trim_end_matches(".jsonl").to_owned(),
                    path: path.to_string_lossy().into_owned(),
                    modified_at: modified,
                    metadata: BTreeMap::from([("project_dir".to_owned(), parent)]),
                })
            })
            .collect())
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let (lines, scan_error) = scan_lines(std::path::Path::new(&session.path), MAX_LINE)
            .map_err(|e| format!("failed to read session file: {e}"))?;
        let (mut project, mut model) = (String::new(), String::new());
        let mut facts = Vec::new();
        for line in lines.iter().filter(|l| !l.trim().is_empty()) {
            let Ok(entry) = super::parse_object::<ClaudeEntry>(line.as_bytes()) else {
                continue;
            };
            if !entry.project.is_empty() {
                project.clone_from(&entry.project);
            }
            if !entry.model.is_empty() {
                model.clone_from(&entry.model);
            }
            let message = entry.message.unwrap_or_default();
            if (entry.kind == "assistant" || message.role == "assistant")
                && message.content.len() > 50
            {
                facts.push(ImportedFact {
                    content: message.content,
                    source: "claude-code".into(),
                    session_id: session.session_id.clone(),
                    timestamp: parse_timestamp(&entry.timestamp),
                    metadata: BTreeMap::from([
                        ("project".to_owned(), project.clone()),
                        ("model".to_owned(), model.clone()),
                    ]),
                    evidence: Vec::new(),
                });
            }
        }
        scan_error.map_or(Ok(facts), Err)
    }
}

/// Imports `~/.codex/archived_sessions/rollout-*.jsonl`.
pub struct Codex {
    custom_path: String,
}

impl Codex {
    /// `custom_path` empty means `$HOME/.codex`.
    #[must_use]
    pub fn new(custom_path: &str) -> Self {
        Self {
            custom_path: custom_path.to_owned(),
        }
    }
}

#[derive(Deserialize, Default)]
struct CodexPayload {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
}

#[derive(Deserialize, Default)]
struct CodexEntry {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    payload: Option<CodexPayload>,
}

impl SessionImporter for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }
    fn is_transcript(&self) -> bool {
        true
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let base = base_or_home(&self.custom_path, &[".codex"])?;
        Ok(walk_files(&base.join("archived_sessions"))
            .into_iter()
            .filter_map(|path| {
                let name = base_name(&path);
                if !name.ends_with(".jsonl")
                    || name == "session_index.jsonl"
                    || !name.starts_with("rollout-")
                {
                    return None;
                }
                let modified = modified_at(&path).filter(|m| *m >= since)?;
                Some(SessionRef {
                    tool: "codex".into(),
                    session_id: name.trim_end_matches(".jsonl").to_owned(),
                    path: path.to_string_lossy().into_owned(),
                    modified_at: modified,
                    metadata: BTreeMap::new(),
                })
            })
            .collect())
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let (lines, scan_error) = scan_lines(std::path::Path::new(&session.path), MAX_LINE)
            .map_err(|e| format!("failed to read session file: {e}"))?;
        let mut facts = Vec::new();
        for line in lines.iter().filter(|l| !l.trim().is_empty()) {
            let Ok(entry) = super::parse_object::<CodexEntry>(line.as_bytes()) else {
                continue;
            };
            let payload = entry.payload.unwrap_or_default();
            if payload.role == "assistant" && payload.content.len() > 50 {
                facts.push(ImportedFact {
                    content: payload.content,
                    source: "codex".into(),
                    session_id: session.session_id.clone(),
                    timestamp: parse_timestamp(&entry.timestamp),
                    ..ImportedFact::default()
                });
            }
        }
        scan_error.map_or(Ok(facts), Err)
    }
}

/// Imports `.aider.chat.history.md` files below the configured roots.
pub struct Aider {
    custom_paths: Vec<String>,
}

impl Aider {
    /// Empty `custom_paths` means `$HOME`.
    #[must_use]
    pub fn new(custom_paths: Vec<String>) -> Self {
        Self { custom_paths }
    }
}

impl SessionImporter for Aider {
    fn name(&self) -> &'static str {
        "aider"
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let roots: Vec<PathBuf> = if self.custom_paths.is_empty() {
            vec![home_dir()?]
        } else {
            self.custom_paths.iter().map(PathBuf::from).collect()
        };
        let mut sessions = Vec::new();
        for root in roots {
            for path in walk_files(&root) {
                if base_name(&path) != ".aider.chat.history.md" {
                    continue;
                }
                let Some(modified) = modified_at(&path).filter(|m| *m >= since) else {
                    continue;
                };
                let dir = dir_name(&path);
                sessions.push(SessionRef {
                    tool: "aider".into(),
                    session_id: dir.clone(),
                    path: path.to_string_lossy().into_owned(),
                    modified_at: modified,
                    metadata: BTreeMap::from([("project".to_owned(), dir)]),
                });
            }
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let bytes =
            std::fs::read(&session.path).map_err(|e| format!("failed to open file: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        let project = session.metadata.get("project").cloned().unwrap_or_default();
        let mut facts = Vec::new();
        let (mut role, mut content) = (String::new(), String::new());
        let mut flush = |role: &str, content: &mut String| {
            if role == "assistant" && content.len() > 50 {
                facts.push(ImportedFact {
                    content: std::mem::take(content),
                    source: "aider".into(),
                    session_id: session.session_id.clone(),
                    timestamp: session.modified_at,
                    metadata: BTreeMap::from([("project".to_owned(), project.clone())]),
                    evidence: Vec::new(),
                });
            }
            content.clear();
        };
        for line in text.lines() {
            if line.starts_with("# ") || line.starts_with("## ") {
                flush(&role, &mut content);
                role.clear();
            } else if line.starts_with("**Human**:") || line.starts_with("**Assistant**:") {
                flush(&role, &mut content);
                role = if line.starts_with("**Human**:") {
                    "human"
                } else {
                    "assistant"
                }
                .into();
            } else if !role.is_empty() {
                content.push_str(line);
                content.push('\n');
            }
        }
        flush(&role, &mut content);
        Ok(facts)
    }
}

#[cfg(test)]
#[path = "transcripts_tests.rs"]
mod tests;
