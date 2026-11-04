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

use crate::load::ReadBudget;
use crate::model::{
    Issue, MAX_INPUT_SIZE, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES, SkillError,
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

#[derive(Debug)]
pub(crate) enum LibraryReadError {
    Io(io::Error),
    InputBound,
}

impl std::fmt::Display for LibraryReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => std::fmt::Display::fmt(error, formatter),
            Self::InputBound => write!(
                formatter,
                "library exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}"
            ),
        }
    }
}

/// Reads entries without collecting beyond the shared bound, then sorts them.
pub(crate) fn read_library_entries(
    library_dir: &Path,
) -> Result<Vec<fs::DirEntry>, LibraryReadError> {
    let entries = fs::read_dir(library_dir).map_err(LibraryReadError::Io)?;
    let mut bounded = Vec::new();
    for entry in entries {
        if bounded.len() >= MAX_RESOURCE_ENTRIES {
            return Err(LibraryReadError::InputBound);
        }
        bounded.push(entry.map_err(LibraryReadError::Io)?);
    }
    bounded.sort_by_key(fs::DirEntry::file_name);
    Ok(bounded)
}

/// Returns library paths in stable order without unbounded directory collection.
///
/// # Errors
///
/// Rejects excessive entry counts or unreadable directories; absence is empty.
pub fn library_paths(library_dir: &Path) -> Result<Vec<PathBuf>, SkillError> {
    match read_library_entries(library_dir) {
        Ok(entries) => Ok(entries.into_iter().map(|entry| entry.path()).collect()),
        Err(LibraryReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Vec::new())
        }
        Err(error) => Err(SkillError(error.to_string())),
    }
}

/// Reads every loadable skill directory in the library.
///
/// Mirrors Go's `skill.ListLibrary`: only real directories are inspected (a
/// symlink to a directory is skipped), each `SKILL.md` is read through the
/// bundle root with `os.Root` semantics and every failure becomes a
/// `skill_load` issue carrying Go's error text. Entries are returned in
/// directory-name order and their categories are canonicalized across the
/// whole library.
#[must_use]
pub fn list_library(library_dir: &Path) -> (Vec<LibraryEntry>, Vec<Issue>) {
    let entries = match read_library_entries(library_dir) {
        Ok(entries) => entries,
        Err(LibraryReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new());
        }
        Err(error) => {
            let (code, message) = match &error {
                LibraryReadError::InputBound => ("library_input_bound", error.to_string()),
                LibraryReadError::Io(source) => (
                    "library_read",
                    format!(
                        "open {}: {}",
                        library_dir.display(),
                        crate::gotext::io_text(source)
                    ),
                ),
            };
            return (
                Vec::new(),
                vec![Issue {
                    code: code.to_owned(),
                    severity: "error".to_owned(),
                    message,
                    path: library_dir.display().to_string(),
                }],
            );
        }
    };

    let mut budget = ReadBudget::new(MAX_TOTAL_RESOURCE_BYTES, "library skill inputs");
    let mut loaded = Vec::new();
    let mut issues = Vec::new();
    for entry in entries {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let issue_path = name.to_string_lossy().into_owned();
        let root = absolute(&crate::gotext::join(library_dir, &name));
        let bytes = match crate::library_go_read::read_skill_md(&root, MAX_INPUT_SIZE) {
            Ok(bytes) => bytes,
            Err(message) => {
                issues.push(load_issue(&message, &issue_path));
                continue;
            }
        };
        if let Err(error) = budget.charge(bytes.len() as u64) {
            issues.push(load_issue(&error, &issue_path));
            break;
        }
        let frontmatter = match crate::model::parse_frontmatter_go(&bytes) {
            Ok(frontmatter) => frontmatter,
            Err(error) => {
                issues.push(load_issue(&error, &issue_path));
                continue;
            }
        };
        loaded.push(LibraryEntry {
            name: frontmatter.name,
            description: frontmatter.description,
            category: normalize_category(&frontmatter.category),
            path: root.display().to_string(),
        });
    }
    canonicalize_categories(&mut loaded);
    (loaded, issues)
}

fn load_issue(error: &impl std::fmt::Display, path: &str) -> Issue {
    Issue {
        code: "skill_load".to_owned(),
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

/// `filepath.Abs`: join the working directory when relative, then clean.
fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    crate::gotext::clean(&joined)
}
