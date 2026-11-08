//! Curated agent-memory importer (ported for #761 from
//! `importer/curatedmemory/curatedmemory.go` at v0.12.0): Claude Code memory
//! directories (`<home>/.claude/projects/**/*.md`, minus project index files)
//! and the Hermes curated files (`<home>/.hermes/memories/{MEMORY,USER}.md`).
//! `home` is an explicit argument; empty means `$HOME`.

use super::runner::truncate_bytes;
use super::util::{base_name, home_dir, modified_at, scan_lines, walk_files};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::pii::go_regex;
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| go_regex(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]"))
}

/// Imports curated memory files of other agents.
pub struct CuratedMemory {
    home: String,
}

impl CuratedMemory {
    /// `home` empty means the process home directory.
    #[must_use]
    pub fn new(home: &str) -> Self {
        Self {
            home: home.to_owned(),
        }
    }

    fn home_dir(&self) -> Result<PathBuf, String> {
        if self.home.is_empty() {
            home_dir()
        } else {
            Ok(PathBuf::from(&self.home))
        }
    }
}

struct Parsed {
    content: String,
    frontmatter: BTreeMap<String, String>,
    links: Vec<String>,
    word_count: usize,
    modified: Option<DateTime<Utc>>,
}

fn parse(path: &Path) -> std::io::Result<Parsed> {
    let (lines, _) = scan_lines(path, 64 * 1024)?;
    let (mut in_front, mut front_lines, mut body) = (false, Vec::<&str>::new(), String::new());
    let mut frontmatter = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line == "---" {
            in_front = true;
        } else if in_front {
            if line == "---" {
                in_front = false;
                for raw in front_lines.drain(..) {
                    let raw = raw.trim();
                    if raw.is_empty() || raw.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = raw.split_once(':') {
                        frontmatter.insert(
                            key.trim().to_owned(),
                            value
                                .trim()
                                .trim_matches(|c| c == '"' || c == '\'')
                                .to_owned(),
                        );
                    }
                }
            } else {
                front_lines.push(line);
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    let content = body.trim().to_owned();
    let mut wiki: Vec<String> = Vec::new();
    for caps in link_re().captures_iter(&content) {
        let link = caps[1].trim().to_owned();
        if !wiki.contains(&link) {
            wiki.push(link);
        }
    }
    Ok(Parsed {
        word_count: content.split_whitespace().count(),
        modified: modified_at(path),
        content,
        frontmatter,
        links: wiki,
    })
}

impl SessionImporter for CuratedMemory {
    fn name(&self) -> &'static str {
        "curated-memory"
    }
    fn category(&self) -> Option<&str> {
        Some("notes")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let home = self.home_dir()?;
        let mut sessions = Vec::new();
        let claude = home.join(".claude").join("projects");
        for path in walk_files(&claude) {
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            // Project index files (`<project>/MEMORY.md` next to a `memory/`
            // directory, or `MEMORY.md` inside one) are skipped.
            if base_name(&path) == "MEMORY.md" {
                let parent = path.parent().unwrap_or(Path::new("."));
                if base_name(parent) == "memory" || parent.join("memory").exists() {
                    continue;
                }
            }
            let Some(modified) = modified_at(&path).filter(|m| *m >= since) else {
                continue;
            };
            let rel = path
                .strip_prefix(&claude)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let project = rel.split('/').next().unwrap_or_default().to_owned();
            sessions.push(SessionRef {
                tool: "curated-memory".into(),
                session_id: format!("claude-code:{rel}"),
                path: path.to_string_lossy().into_owned(),
                modified_at: modified,
                metadata: BTreeMap::from([
                    ("source_tool".to_owned(), "claude-code".to_owned()),
                    ("project".to_owned(), project),
                ]),
            });
        }
        let hermes = home.join(".hermes").join("memories");
        for name in ["MEMORY.md", "USER.md"] {
            let path = hermes.join(name);
            let Some(modified) = modified_at(&path).filter(|m| *m >= since) else {
                continue;
            };
            sessions.push(SessionRef {
                tool: "curated-memory".into(),
                session_id: format!("hermes:{name}"),
                path: path.to_string_lossy().into_owned(),
                modified_at: modified,
                metadata: BTreeMap::from([("source_tool".to_owned(), "hermes".to_owned())]),
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let file = parse(Path::new(&session.path))
            .map_err(|e| format!("failed to read memory file: {e}"))?;
        if file.content.is_empty() {
            return Ok(Vec::new());
        }
        let content = if file.content.len() > 5000 {
            format!("{}...", truncate_bytes(&file.content, 5000))
        } else {
            file.content.clone()
        };
        let modified = file.modified.unwrap_or_else(crate::sync_state::zero_time);
        let mut metadata = BTreeMap::from([
            ("source".to_owned(), session.tool.clone()),
            ("file_path".to_owned(), session.session_id.clone()),
            ("word_count".to_owned(), file.word_count.to_string()),
            (
                "modified".to_owned(),
                modified.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            ),
        ]);
        if let Some(project) = session.metadata.get("project").filter(|p| !p.is_empty()) {
            metadata.insert("project".into(), project.clone());
        }
        if !file.frontmatter.is_empty() {
            for (key, target) in [
                ("name", "title"),
                ("description", "description"),
                ("type", "memory_type"),
            ] {
                if let Some(value) = file.frontmatter.get(key) {
                    metadata.insert(target.into(), value.clone());
                }
            }
            metadata.insert(
                "frontmatter".into(),
                serde_json::to_string(&file.frontmatter).unwrap_or_default(),
            );
        }
        if !file.links.is_empty() {
            metadata.insert(
                "links".into(),
                serde_json::to_string(&file.links).unwrap_or_default(),
            );
        }
        Ok(vec![ImportedFact {
            content,
            source: session.tool.clone(),
            session_id: session.session_id.clone(),
            timestamp: modified,
            metadata,
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::test_support::{TestDir, touch};
    use chrono::TimeZone;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 6, 0, 0).unwrap()
    }

    fn home() -> TestDir {
        let dir = TestDir::new("curated");
        let files = [
            (
                ".claude/projects/proj-a/memory/feedback.md",
                "---\nname: \"Style\"\ntype: feedback\ndescription: d\n---\nPrefer short answers. See [[Alpha]] and [[Alpha|a]] and [[ Beta ]].\n",
                5,
            ),
            (
                ".claude/projects/proj-a/memory/MEMORY.md",
                "inside memory dir",
                5,
            ),
            (
                ".claude/projects/proj-a/MEMORY.md",
                "index next to memory dir",
                5,
            ),
            (
                ".claude/projects/proj-b/MEMORY.md",
                "standalone memory file",
                5,
            ),
            (".claude/projects/proj-b/notes.txt", "not markdown", 5),
            (".claude/projects/proj-c/memory/old.md", "old", 1),
            (".hermes/memories/MEMORY.md", "Hermes memory", 6),
            (".hermes/memories/USER.md", "", 6),
            (".hermes/memories/OTHER.md", "ignored", 6),
        ];
        for (rel, body, day) in files {
            touch(&dir.write(rel, body), at(day));
        }
        dir
    }

    #[test]
    fn discovery_covers_both_profiles() {
        let dir = home();
        let importer = CuratedMemory::new(&dir.path().to_string_lossy());
        let sessions = importer.discover_sessions(at(3)).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "claude-code:proj-a/memory/feedback.md",
                "claude-code:proj-b/MEMORY.md",
                "hermes:MEMORY.md",
                "hermes:USER.md"
            ]
        );
        assert_eq!(sessions[0].metadata["project"], "proj-a");
        assert_eq!(sessions[0].metadata["source_tool"], "claude-code");
        assert_eq!(sessions[2].metadata["source_tool"], "hermes");
        assert!(sessions.iter().all(|s| s.tool == "curated-memory"));
        assert_eq!(
            importer.discover_sessions(at(1)).unwrap().len(),
            5,
            "old.md joins with an earlier cursor"
        );
    }

    #[test]
    fn imports_with_frontmatter_links_and_empty_files() {
        let dir = home();
        let importer = CuratedMemory::new(&dir.path().to_string_lossy());
        let sessions = importer.discover_sessions(at(3)).unwrap();
        let fact = &importer.import_session(&sessions[0]).unwrap()[0];
        assert_eq!(
            fact.content,
            "Prefer short answers. See [[Alpha]] and [[Alpha|a]] and [[ Beta ]]."
        );
        assert_eq!(fact.source, "curated-memory");
        assert_eq!(fact.metadata["title"], "Style");
        assert_eq!(fact.metadata["memory_type"], "feedback");
        assert_eq!(fact.metadata["project"], "proj-a");
        assert_eq!(fact.metadata["links"], "[\"Alpha\",\"Beta\"]");
        assert_eq!(
            fact.metadata["frontmatter"],
            "{\"description\":\"d\",\"name\":\"Style\",\"type\":\"feedback\"}"
        );
        assert_eq!(
            fact.metadata["file_path"],
            "claude-code:proj-a/memory/feedback.md"
        );
        assert_eq!(fact.timestamp, at(5));
        assert!(
            importer.import_session(&sessions[3]).unwrap().is_empty(),
            "empty USER.md"
        );
        let missing = SessionRef {
            path: dir.path().join("nope.md").to_string_lossy().into_owned(),
            ..SessionRef::default()
        };
        assert!(
            importer
                .import_session(&missing)
                .unwrap_err()
                .starts_with("failed to read memory file: ")
        );
        let empty_home = TestDir::new("curated-empty");
        assert!(
            CuratedMemory::new(&empty_home.path().to_string_lossy())
                .discover_sessions(at(1))
                .unwrap()
                .is_empty()
        );
    }
}
