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
use std::path::{Path, PathBuf};

use ambient_authority::ambient_authority;
use cap_std::fs::Dir;

use crate::load::{ReadBudget, read_skill_document};
use crate::model::{
    Issue, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError, parse_skill_md,
};

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

/// Reads directory entries without collecting beyond the shared resource-entry
/// bound. The returned entries are sorted for stable diagnostics and results.
pub(crate) fn read_library_entries(library_dir: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let entries = fs::read_dir(library_dir)?;
    let mut bounded = Vec::new();
    for entry in entries {
        if bounded.len() >= MAX_RESOURCE_ENTRIES {
            return Err(io::Error::other(format!(
                "library exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}"
            )));
        }
        bounded.push(entry?);
    }
    bounded.sort_by_key(fs::DirEntry::file_name);
    Ok(bounded)
}

/// Reads every loadable skill directory in the library.
///
/// Non-directory entries are ignored, exactly like the Go loader. Entries are
/// returned in directory-name order and their categories are canonicalized
/// across the whole library.
#[must_use]
pub fn list_library(library_dir: &Path) -> (Vec<LibraryEntry>, Vec<Issue>) {
    let entries = match read_library_entries(library_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => {
            let bounded = error.to_string().contains("maximum entry count");
            return (
                Vec::new(),
                vec![Issue {
                    code: if bounded {
                        "library_input_bound"
                    } else {
                        "library_read"
                    }
                    .to_owned(),
                    severity: "error".to_owned(),
                    message: error.to_string(),
                    path: library_dir.display().to_string(),
                }],
            );
        }
    };
    let library_cap = match Dir::open_ambient_dir(library_dir, ambient_authority()) {
        Ok(directory) => directory,
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

    let mut budget = ReadBudget::new(MAX_TOTAL_RESOURCE_BYTES, "library skill inputs");
    let mut loaded = Vec::new();
    let mut issues = Vec::new();
    for entry in entries {
        let root = entry.path();
        if !root.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let issue_path = name.to_string_lossy().into_owned();
        let relative = PathBuf::from(&name).join("SKILL.md");
        let bytes = match read_skill_document(
            &library_cap,
            &relative,
            &format!("{issue_path}/SKILL.md"),
            Some(&mut budget),
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                let stop = error
                    .0
                    .contains("library skill inputs exceeds maximum total size");
                issues.push(load_issue(&error, &issue_path));
                if stop {
                    break;
                }
                continue;
            }
        };
        let parsed = match parse_skill_md(&bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(load_issue(&error, &issue_path));
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

fn load_issue(error: &SkillError, path: &str) -> Issue {
    let bounded_or_special = error.0.contains("exceeds maximum")
        || error.0.contains("must be a regular file")
        || error.0.contains("escapes skill root");
    Issue {
        code: if bounded_or_special {
            "skill_input_rejected"
        } else {
            "skill_load"
        }
        .to_owned(),
        severity: "error".to_owned(),
        message: error.to_string(),
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
