//! Obsidian vault importer (ported for #761 from
//! `importer/obsidian/obsidian.go` at v0.12.0): markdown notes below a vault
//! directory the caller names (an empty path auto-detects `OBSIDIAN_VAULT` or
//! the iCloud `LifeOS` vault, see [`detect_vault_path`]). Note bodies are cut at
//! 5000 bytes on a character boundary.

use super::runner::truncate_bytes;
use super::util::{base_name, home_dir, modified_at, walk_files};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::pii::go_regex;
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_EXCLUDES: [&str; 4] = [".obsidian", ".trash", "Templates", "node_modules"];

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| go_regex(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]"))
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| go_regex(r"#([a-zA-Z0-9_/\-]+)"))
}

/// Imports notes of one vault.
pub struct Obsidian {
    vault_path: String,
    tags: Vec<String>,
    exclude_folders: Vec<String>,
    exclude_tags: Vec<String>,
    max_note_length: usize,
}

struct Note {
    title: String,
    content: String,
    tags: Vec<String>,
    wikilinks: Vec<String>,
    frontmatter: BTreeMap<String, String>,
    modified: Option<DateTime<Utc>>,
    word_count: usize,
}

/// `detectVaultPath`: `OBSIDIAN_VAULT` when it exists, else the iCloud `LifeOS`
/// vault below `home`, else empty.
#[must_use]
pub fn detect_vault_path(env_vault: Option<&str>, home: Option<&Path>) -> String {
    if let Some(path) = env_vault.filter(|p| !p.is_empty() && Path::new(p).exists()) {
        return path.to_owned();
    }
    let Some(home) = home else {
        return String::new();
    };
    let life = home.join("Library/Mobile Documents/iCloud~md~obsidian/Documents/LifeOS");
    if life.exists() {
        life.to_string_lossy().into_owned()
    } else {
        String::new()
    }
}

impl Obsidian {
    /// `folder` is accepted for signature parity (Go stored it but never used it).
    #[must_use]
    pub fn new(
        vault_path: &str,
        _folder: &str,
        tags: Vec<String>,
        exclude_folders: Vec<String>,
        exclude_tags: Vec<String>,
    ) -> Self {
        let vault_path = if vault_path.is_empty() {
            detect_vault_path(
                std::env::var("OBSIDIAN_VAULT").ok().as_deref(),
                home_dir().ok().as_deref(),
            )
        } else {
            vault_path.to_owned()
        };
        Self {
            vault_path,
            tags,
            exclude_folders,
            exclude_tags,
            max_note_length: 5000,
        }
    }

    fn excluded_folder(&self, rel: &str) -> bool {
        DEFAULT_EXCLUDES
            .iter()
            .copied()
            .chain(self.exclude_folders.iter().map(String::as_str))
            .any(|ex| {
                rel.starts_with(&format!("{ex}/")) || rel.contains(&format!("/{ex}/")) || rel == ex
            })
    }
}

fn dedup_matches(re: &Regex, content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for caps in re.captures_iter(content) {
        let value = caps[1].to_owned();
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn parse_note(path: &Path) -> std::io::Result<Note> {
    let (lines, _) = super::util::scan_lines(path, 64 * 1024)?;
    let (mut in_front, mut front_lines, mut body) = (false, Vec::<String>::new(), String::new());
    let mut frontmatter = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line == "---" {
            in_front = true;
        } else if in_front {
            if line == "---" {
                in_front = false;
                for raw in &front_lines {
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
                front_lines.push(line.clone());
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    let content = body.trim().to_owned();
    let title = content
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("# "))
        .map_or_else(
            || base_name(path).trim_end_matches(".md").to_owned(),
            str::to_owned,
        );
    Ok(Note {
        title,
        tags: dedup_matches(tag_re(), &content),
        wikilinks: dedup_matches(wikilink_re(), &content),
        word_count: content.split_whitespace().count(),
        modified: modified_at(path),
        frontmatter,
        content,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

impl SessionImporter for Obsidian {
    fn name(&self) -> &'static str {
        "obsidian"
    }
    fn category(&self) -> Option<&str> {
        Some("notes")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Internal)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        if self.vault_path.is_empty() {
            return Err("no vault path configured".into());
        }
        let root = PathBuf::from(&self.vault_path);
        let mut sessions = Vec::new();
        for path in walk_files(&root) {
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if self.excluded_folder(&rel) {
                continue;
            }
            let Some(modified) = modified_at(&path).filter(|m| *m >= since) else {
                continue;
            };
            let Ok(note) = parse_note(&path) else {
                continue;
            };
            if !self.tags.is_empty() && !note.tags.iter().any(|t| self.tags.contains(t)) {
                continue;
            }
            if note.tags.iter().any(|t| self.exclude_tags.contains(t)) {
                continue;
            }
            let mut metadata = BTreeMap::from([
                ("title".to_owned(), note.title.clone()),
                ("vault".to_owned(), base_name(&root)),
                ("word_count".to_owned(), note.word_count.to_string()),
            ]);
            if !note.tags.is_empty() {
                metadata.insert("tags".into(), to_json(&note.tags));
            }
            sessions.push(SessionRef {
                tool: "obsidian".into(),
                session_id: rel,
                path: path.to_string_lossy().into_owned(),
                modified_at: modified,
                metadata,
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let note = parse_note(Path::new(&session.path))
            .map_err(|e| format!("failed to parse note: {e}"))?;
        let content = if note.content.len() > self.max_note_length {
            format!("{}...", truncate_bytes(&note.content, self.max_note_length))
        } else {
            note.content.clone()
        };
        let modified = note.modified.unwrap_or_else(crate::sync_state::zero_time);
        let mut metadata = BTreeMap::from([
            ("source".to_owned(), "obsidian".to_owned()),
            ("vault".to_owned(), base_name(Path::new(&self.vault_path))),
            ("note_path".to_owned(), session.session_id.clone()),
            ("title".to_owned(), note.title.clone()),
            ("word_count".to_owned(), note.word_count.to_string()),
            (
                "modified".to_owned(),
                modified.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            ),
        ]);
        if !note.tags.is_empty() {
            metadata.insert("tags".into(), to_json(&note.tags));
        }
        if !note.wikilinks.is_empty() {
            metadata.insert("wikilinks".into(), to_json(&note.wikilinks));
        }
        if !note.frontmatter.is_empty() {
            metadata.insert("frontmatter".into(), to_json(&note.frontmatter));
        }
        Ok(vec![ImportedFact {
            content: format!("Note: {}\n\n{content}", note.title),
            source: "obsidian".into(),
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
        Utc.with_ymd_and_hms(2026, 9, day, 7, 0, 0).unwrap()
    }

    fn vault() -> TestDir {
        let dir = TestDir::new("obsidian");
        let notes = [
            (
                "Plans/roadmap.md",
                "---\ntitle: \"Road\"\nowner: 'ann'\n# comment\n---\n# Roadmap 2026\nSee [[Alpha]] and [[Beta|the beta]] again [[Alpha]].\n#project #q3/goals #project\n",
                20,
            ),
            ("daily.md", "Just words here\n", 20),
            ("Templates/tpl.md", "# tpl", 20),
            ("sub/.trash/x.md", "gone", 20),
            ("private/secret.md", "#secret note", 20),
            ("old.md", "# Old", 1),
            ("readme.txt", "ignored", 20),
        ];
        for (rel, body, day) in notes {
            let path = dir.write(&format!("vault/{rel}"), body);
            touch(&path, at(day));
        }
        dir
    }

    #[test]
    fn discovery_filters_and_metadata() {
        let dir = vault();
        let root = dir.path().join("vault").to_string_lossy().into_owned();
        let importer = Obsidian::new(
            &root,
            "",
            Vec::new(),
            vec!["private".into()],
            vec!["secret".into()],
        );
        let sessions = importer.discover_sessions(at(10)).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, ["Plans/roadmap.md", "daily.md"]);
        assert_eq!(sessions[0].metadata["title"], "Roadmap 2026");
        assert_eq!(sessions[0].metadata["vault"], "vault");
        assert_eq!(sessions[0].metadata["tags"], "[\"project\",\"q3/goals\"]");
        assert_eq!(sessions[1].metadata["title"], "daily");
        assert!(!sessions[1].metadata.contains_key("tags"));
        let tagged = Obsidian::new(&root, "", vec!["q3/goals".into()], Vec::new(), Vec::new());
        assert_eq!(tagged.discover_sessions(at(1)).unwrap().len(), 1);
        let excluded_by_tag = Obsidian::new(
            &root,
            "",
            Vec::new(),
            vec!["private".into()],
            vec!["project".into()],
        );
        assert_eq!(
            excluded_by_tag.discover_sessions(at(1)).unwrap().len(),
            2,
            "daily + old"
        );
    }

    #[test]
    fn imports_a_note() {
        let dir = vault();
        let root = dir.path().join("vault").to_string_lossy().into_owned();
        let importer = Obsidian::new(&root, "", Vec::new(), Vec::new(), Vec::new());
        let session = importer.discover_sessions(at(10)).unwrap().remove(0);
        let fact = &importer.import_session(&session).unwrap()[0];
        assert!(
            fact.content
                .starts_with("Note: Roadmap 2026\n\n# Roadmap 2026\nSee [[Alpha]]"),
            "{}",
            fact.content
        );
        assert_eq!(fact.metadata["wikilinks"], "[\"Alpha\",\"Beta\"]");
        assert_eq!(
            fact.metadata["frontmatter"],
            "{\"owner\":\"ann\",\"title\":\"Road\"}"
        );
        assert_eq!(fact.metadata["modified"], "2026-09-20T07:00:00Z");
        assert_eq!(fact.metadata["note_path"], "Plans/roadmap.md");
        assert_eq!(fact.timestamp, at(20));
        let bad = SessionRef {
            path: dir.path().join("none.md").to_string_lossy().into_owned(),
            ..SessionRef::default()
        };
        assert!(
            importer
                .import_session(&bad)
                .unwrap_err()
                .starts_with("failed to parse note: ")
        );
    }

    #[test]
    fn long_notes_are_cut_and_vault_detection() {
        let dir = TestDir::new("obsidian-long");
        let path = dir.write("v/n.md", &"é".repeat(3000));
        touch(&path, at(5));
        let root = dir.path().join("v").to_string_lossy().into_owned();
        let importer = Obsidian::new(&root, "", Vec::new(), Vec::new(), Vec::new());
        let session = importer.discover_sessions(at(1)).unwrap().remove(0);
        let fact = &importer.import_session(&session).unwrap()[0];
        assert!(fact.content.ends_with("é..."));
        assert_eq!(fact.content.len(), "Note: n\n\n".len() + 5000 + 3);
        assert_eq!(
            Obsidian {
                vault_path: String::new(),
                ..importer
            }
            .discover_sessions(at(1))
            .unwrap_err(),
            "no vault path configured"
        );
        let home = dir.path().join("home");
        let life = home.join("Library/Mobile Documents/iCloud~md~obsidian/Documents/LifeOS");
        std::fs::create_dir_all(&life).unwrap();
        assert_eq!(detect_vault_path(None, Some(&home)), life.to_string_lossy());
        assert_eq!(detect_vault_path(Some(&root), Some(&home)), root);
        assert_eq!(detect_vault_path(Some("/does/not/exist"), None), "");
    }
}
