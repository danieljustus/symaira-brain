//! Library inventory behind `symbrain skills list`.
//!
//! Each bundle has a confined root and the shared bounded read budget. Ordinary
//! read errors retain Go's operation/relative document label; resource-bound
//! and special-file refusals keep the pinned corrective contracts.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ambient_authority::ambient_authority;
use cap_std::fs::Dir;

use crate::load::{InputReadError, ReadBudget, read_skill_document};
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
    pub path: PathBuf,
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
/// Non-directory entries are ignored, exactly like the Go loader. Entries are
/// returned in directory-name order and their categories are canonicalized
/// across the whole library.
#[must_use]
pub fn list_library(library_dir: &Path) -> (Vec<LibraryEntry>, Vec<Issue>) {
    let entries = match read_library_entries(library_dir) {
        Ok(entries) => entries,
        Err(LibraryReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new());
        }
        Err(error) => {
            let bounded = matches!(error, LibraryReadError::InputBound);
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
                    message: match &error {
                        LibraryReadError::Io(error) => {
                            crate::io_contract::path_error("open", library_dir, error)
                        }
                        LibraryReadError::InputBound => error.to_string().into(),
                    },
                    path: crate::GoText::from_path(library_dir),
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
                    message: crate::io_contract::path_error("open", library_dir, &error),
                    path: crate::GoText::from_path(library_dir),
                }],
            );
        }
    };

    let mut budget = ReadBudget::new(MAX_TOTAL_RESOURCE_BYTES, "library skill inputs");
    let mut loaded = Vec::new();
    let mut issues = Vec::new();
    for entry in entries {
        let root = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let issue_path = crate::GoText::from_path(Path::new(&name));
        let bundle_cap = match library_cap.open_dir(&name) {
            Ok(directory) => directory,
            Err(error) => {
                let message = crate::io_contract::path_error("open", &root, &error)
                    .prefixed("open skill root: ");
                issues.push(Issue {
                    code: "skill_load".into(),
                    severity: "error".into(),
                    message,
                    path: issue_path.clone(),
                });
                continue;
            }
        };
        let bytes = match read_skill_document(
            &bundle_cap,
            Path::new("SKILL.md"),
            "SKILL.md",
            Some(&mut budget),
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                let stop = matches!(error, InputReadError::Budget(_));
                let rejected = !matches!(error, InputReadError::Read(_));
                issues.push(load_issue(&error, &issue_path, rejected));
                if stop {
                    break;
                }
                continue;
            }
        };
        let parsed = match parse_skill_md(&bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(load_issue(&error, &issue_path, false));
                continue;
            }
        };
        let frontmatter = parsed.frontmatter;
        loaded.push(LibraryEntry {
            name: frontmatter.name,
            description: frontmatter.description,
            category: normalize_category(&frontmatter.category),
            path: std::path::absolute(&root).unwrap_or(root),
        });
    }
    canonicalize_categories(&mut loaded);
    (loaded, issues)
}

fn load_issue(error: &impl std::fmt::Display, path: &crate::GoText, rejected: bool) -> Issue {
    Issue {
        code: if rejected {
            "skill_input_rejected"
        } else {
            "skill_load"
        }
        .to_owned(),
        severity: "error".to_owned(),
        message: error.to_string().into(),
        path: path.clone(),
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
