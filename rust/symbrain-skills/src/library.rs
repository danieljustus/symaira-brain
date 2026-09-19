//! Library inventory behind `symbrain skills list`.
//!
//! The native slice deliberately covers libraries whose entries all load
//! cleanly: Go reports a per-entry issue with cap-std error text when a
//! `SKILL.md` is missing, unreadable or malformed, and that text is not
//! reproducible here. [`super::install`] callers therefore keep such libraries
//! on Go (see the CLI fallback gate) — the issue shape below exists so the
//! report type is complete, not as a byte-compatible error path.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::model::{Issue, parse_skill_md};

/// One library skill, as `skills list` reports it.
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    /// Frontmatter name.
    pub name: String,
    /// Frontmatter description.
    pub description: String,
    /// Canonicalized frontmatter category.
    pub category: String,
    /// Absolute skill directory.
    pub path: String,
}

/// Reads every loadable skill directory in the library.
///
/// Non-directory entries are ignored, exactly like the Go loader. Entries are
/// returned in directory-name order and their categories are canonicalized
/// across the whole library.
#[must_use]
pub fn list_library(library_dir: &Path) -> (Vec<LibraryEntry>, Vec<Issue>) {
    let entries = match fs::read_dir(library_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => {
            return (
                Vec::new(),
                vec![Issue {
                    code: "library_read".to_owned(),
                    severity: "error".to_owned(),
                    message: error.to_string(),
                    path: library_dir.display().to_string(),
                }],
            );
        }
    };

    let mut names = entries
        .flatten()
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    names.sort();
    let mut loaded = Vec::new();
    let mut issues = Vec::new();
    for name in names {
        let root = library_dir.join(&name);
        if !root.is_dir() {
            continue;
        }
        let issue_path = name.to_string_lossy().into_owned();
        let bytes = match fs::read(root.join("SKILL.md")) {
            Ok(bytes) => bytes,
            Err(error) => {
                issues.push(load_issue(&error.to_string(), &issue_path));
                continue;
            }
        };
        let parsed = match parse_skill_md(&bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(load_issue(&error.0, &issue_path));
                continue;
            }
        };
        let frontmatter = parsed.frontmatter;
        loaded.push(LibraryEntry {
            name: frontmatter.name,
            description: frontmatter.description,
            category: normalize_category(&frontmatter.category),
            path: absolute(&root),
        });
    }
    canonicalize_categories(&mut loaded);
    (loaded, issues)
}

fn load_issue(message: &str, path: &str) -> Issue {
    Issue {
        code: "skill_load".to_owned(),
        severity: "error".to_owned(),
        message: message.to_owned(),
        path: path.to_owned(),
    }
}

/// Canonicalizes category spelling across a loaded library: the first
/// non-empty spelling wins, later case-insensitive variants reuse it.
fn canonicalize_categories(entries: &mut [LibraryEntry]) {
    let mut canonical: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries {
        let category = normalize_category(&entry.category);
        if category.is_empty() {
            entry.category = String::new();
            continue;
        }
        let key = category.to_lowercase();
        if let Some(existing) = canonical.get(&key) {
            entry.category.clone_from(existing);
        } else {
            canonical.insert(key, category.clone());
            entry.category = category;
        }
    }
}

/// Trims and collapses category whitespace without changing the spelling.
fn normalize_category(category: &str) -> String {
    category.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
