//! Codex Markdown memory importer (ported for #761 from
//! `importer/codexmemory/codexmemory.go` at v0.12.0).
//!
//! Imports the curated Markdown tier Codex writes below `~/.codex/memories`
//! (`MEMORY.md`, `memory_summary.md`, `raw_memories.md`, `rollout_summaries/`
//! and `extensions/<name>/resources/`), never rollout JSONL. The root is an
//! explicit argument (empty = `$HOME/.codex/memories`). Facts are staged,
//! untrusted and carry exact-span evidence; a 6h summary suppresses the 10min
//! files it covers; an application allow/deny policy filters by frontmatter.

use super::markdown::{parse_applications, parse_markdown, read_markdown, section_body};
use super::util::{base_name, home_dir, modified_at, walk_files};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::evidence::{Extraction, SourceRef, Span};
use crate::pii::go_regex;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const NAME: &str = "codex-memory";
const PROMOTION_POLICY: &str = "recurring_only;prefer_6h;single_10min_not_promotable";

/// Allow/deny lists of application bundle ids; deny wins.
#[derive(Debug, Clone, Default)]
pub struct ApplicationPolicy {
    /// When non-empty, at least one application must match.
    pub allowed: Vec<String>,
    /// Any match rejects the document.
    pub denied: Vec<String>,
}

/// Imports the Codex Markdown memory tier read-only.
pub struct CodexMemory {
    root: String,
    allow: BTreeSet<String>,
    deny: BTreeSet<String>,
    last_import: Mutex<Option<DateTime<Utc>>>,
}

fn resource_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        go_regex(r"^(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2})-[^-]+-(10min|6h)-(.+)\.md$")
    })
}

fn set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

fn parse_resource_name(base: &str) -> Option<(&'static str, DateTime<Utc>)> {
    let caps = resource_re().captures(base)?;
    let start = NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%dT%H-%M-%S")
        .ok()?
        .and_utc();
    Some((if &caps[2] == "6h" { "6h" } else { "10min" }, start))
}

fn activity_duration(granularity: &str) -> Duration {
    if granularity == "6h" {
        Duration::hours(6)
    } else {
        Duration::minutes(10)
    }
}

fn rfc(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl CodexMemory {
    /// `root` empty means `$HOME/.codex/memories`.
    #[must_use]
    pub fn new(root: &str, policy: &ApplicationPolicy) -> Self {
        Self {
            root: root.to_owned(),
            allow: set(&policy.allowed),
            deny: set(&policy.denied),
            last_import: Mutex::new(None),
        }
    }

    fn root_dir(&self) -> Result<PathBuf, String> {
        if self.root.is_empty() {
            Ok(home_dir()?.join(".codex").join("memories"))
        } else {
            Ok(PathBuf::from(&self.root))
        }
    }

    fn application_allowed(&self, path: &Path) -> bool {
        if self.allow.is_empty() && self.deny.is_empty() {
            return true;
        }
        let Ok(text) = read_markdown(path) else {
            return false;
        };
        let apps = parse_applications(
            parse_markdown(&text)
                .0
                .get("applications")
                .map_or("", String::as_str),
        );
        if apps.iter().any(|a| self.deny.contains(a)) {
            return false;
        }
        self.allow.is_empty() || apps.iter().any(|a| self.allow.contains(a))
    }

    fn discover_paths(root: &Path) -> Vec<PathBuf> {
        let mut paths: BTreeSet<PathBuf> = ["MEMORY.md", "memory_summary.md", "raw_memories.md"]
            .iter()
            .map(|f| root.join(f))
            .filter(|p| p.is_file())
            .collect();
        for path in walk_files(root) {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let parts: Vec<String> = rel
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let md = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"));
            if md
                && ((parts.len() == 2 && parts[0] == "rollout_summaries")
                    || (parts.len() >= 4 && parts[0] == "extensions" && parts[2] == "resources"))
            {
                paths.insert(path);
            }
        }
        paths.into_iter().collect()
    }
}

impl SessionImporter for CodexMemory {
    fn name(&self) -> &'static str {
        NAME
    }
    fn category(&self) -> Option<&str> {
        Some("activity")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn is_transcript(&self) -> bool {
        true
    }
    fn stage_imported_facts(&self) -> bool {
        true
    }
    fn content_is_untrusted(&self) -> bool {
        true
    }
    fn last_import_time(&self) -> ImportResult<Option<DateTime<Utc>>> {
        Ok(*self.last_import.lock().map_err(|e| e.to_string())?)
    }
    fn mark_imported(&self, session: &SessionRef) -> ImportResult<()> {
        let mut last = self.last_import.lock().map_err(|e| e.to_string())?;
        if last.is_none_or(|l| session.modified_at > l) {
            *last = Some(session.modified_at);
        }
        Ok(())
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let root = self.root_dir()?;
        if !root.exists() {
            return Ok(Vec::new());
        }
        let paths = Self::discover_paths(&root);
        // 6h names count even when their mtime predates the cursor, so a
        // recent 10min file is not re-emitted next to its parent.
        let six_hour: Vec<DateTime<Utc>> = paths
            .iter()
            .filter_map(|p| match parse_resource_name(&base_name(p)) {
                Some(("6h", start)) if self.application_allowed(p) => Some(start),
                _ => None,
            })
            .collect();
        let mut sessions = Vec::new();
        for path in &paths {
            let Some(modified) = modified_at(path).filter(|m| *m >= since) else {
                continue;
            };
            if !self.application_allowed(path) {
                continue;
            }
            let mut metadata = BTreeMap::from([
                ("source_kind".to_owned(), "codex_markdown".to_owned()),
                ("file_path".to_owned(), path.to_string_lossy().into_owned()),
                ("sync_exclude".to_owned(), "true".to_owned()),
                ("promotion_policy".to_owned(), PROMOTION_POLICY.to_owned()),
            ]);
            if let Some((granularity, start)) = parse_resource_name(&base_name(path)) {
                let rel: Vec<String> = path
                    .strip_prefix(&root)
                    .map(|r| r.iter().map(|p| p.to_string_lossy().into_owned()).collect())
                    .unwrap_or_default();
                if rel.len() >= 2 && rel[0] == "extensions" {
                    metadata.insert("extension".into(), rel[1].clone());
                } else {
                    metadata.insert("extension".into(), String::new());
                }
                metadata.insert("granularity".into(), granularity.into());
                metadata.insert("activity_started_at".into(), rfc(start));
                metadata.insert(
                    "activity_ends_at".into(),
                    rfc(start + activity_duration(granularity)),
                );
                metadata.insert(
                    "promotable".into(),
                    if granularity == "10min" {
                        "false"
                    } else {
                        "candidate"
                    }
                    .into(),
                );
                if granularity == "10min"
                    && six_hour
                        .iter()
                        .any(|p| start >= *p && start < *p + Duration::hours(6))
                {
                    continue;
                }
            } else {
                metadata.insert("granularity".into(), "consolidated".into());
            }
            let Ok(rel) = path.strip_prefix(&root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let rel = rel.to_string_lossy().replace('\\', "/");
            sessions.push(SessionRef {
                tool: NAME.into(),
                session_id: format!("{NAME}:{rel}"),
                path: path.to_string_lossy().into_owned(),
                modified_at: modified,
                metadata,
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let path = Path::new(&session.path);
        if base_name(path).eq_ignore_ascii_case("instructions.md") {
            return Ok(Vec::new());
        }
        let root = self.root_dir()?;
        if path.strip_prefix(&root).is_err() || !self.application_allowed(path) {
            return Ok(Vec::new());
        }
        let text = read_markdown(path)
            .map_err(|e| format!("read codex memory {:?}: {e}", session.path))?;
        let (front, body) = parse_markdown(&text);
        let body = body.trim().to_owned();
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let timestamp = if session.modified_at > crate::sync_state::zero_time() {
            session.modified_at
        } else {
            modified_at(path)
                .ok_or_else(|| format!("stat codex memory {:?}: unreadable", session.path))?
        };
        let mut metadata = session.metadata.clone();
        let field = |k: &str| front.get(k).cloned().unwrap_or_default();
        metadata.insert("title".into(), field("title"));
        metadata.insert("description".into(), field("description"));
        if !front.is_empty()
            && let Ok(encoded) = serde_json::to_string(&front)
        {
            metadata.insert("frontmatter".into(), encoded);
        }
        let apps = parse_applications(&field("applications"));
        if !apps.is_empty() {
            metadata.insert("applications".into(), apps.join(","));
        }
        metadata.insert("source_path".into(), session.path.clone());
        let citations = section_body(&body, "Citations");
        if !citations.is_empty() {
            metadata.insert("citations".into(), citations);
        }
        if let Some((granularity, start)) = parse_resource_name(&base_name(path)) {
            metadata.insert("granularity".into(), granularity.into());
            metadata.insert("activity_started_at".into(), rfc(start));
            metadata.insert(
                "activity_ends_at".into(),
                rfc(start + activity_duration(granularity)),
            );
            metadata.insert(
                "promotable".into(),
                if granularity == "10min" {
                    "false"
                } else {
                    "candidate"
                }
                .into(),
            );
        }
        for (key, default) in [
            ("sync_exclude", "true"),
            ("promotion_policy", PROMOTION_POLICY),
        ] {
            if metadata.get(key).is_none_or(String::is_empty) {
                metadata.insert(key.into(), default.into());
            }
        }
        let len = i64::try_from(body.len()).unwrap_or(i64::MAX);
        Ok(vec![ImportedFact {
            content: body.clone(),
            source: NAME.into(),
            session_id: session.session_id.clone(),
            timestamp,
            metadata,
            evidence: vec![Extraction {
                source: SourceRef {
                    id: session.session_id.clone(),
                    kind: NAME.into(),
                },
                text: body.clone(),
                evidence_text: body,
                span: Span { start: 0, end: len },
                alignment_status: "exact".into(),
                attributes: BTreeMap::new(),
            }],
        }])
    }
}

#[cfg(test)]
#[path = "codexmemory_tests.rs"]
mod tests;
