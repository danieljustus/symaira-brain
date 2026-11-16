//! Duplicate skill-name scan across a target's discovery roots.
//!
//! Strictly read-only: it lists directories and stats marker files, nothing
//! is created, renamed or removed. A skill reachable through two roots (for
//! example a symlink-mode install) is one copy, identified by its canonical
//! path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::harness_profile::{HarnessProfile, RootBase};
use crate::targets_status::is_managed_skill;

/// Directory depth followed below a root when the harness descends.
const MAX_DEPTH: usize = 3;

/// One on-disk copy of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillCopy {
    /// Directory holding `SKILL.md`, as found below the scanned root.
    pub path: String,
    /// `user`, `project` or `external`.
    pub scope: String,
    /// Whether the copy carries a symskills install marker.
    pub managed: bool,
}

/// A skill name found more than once for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Duplicate {
    /// Skill name (its directory name).
    pub name: String,
    /// Every copy, user root first, then project, then external roots.
    pub copies: Vec<SkillCopy>,
    /// Which copy the harness loads. `unknown`: no harness precedence is
    /// verified in the profile table, so none is guessed.
    pub harness_uses: String,
}

/// One discovery root to scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// Directory to scan.
    pub path: PathBuf,
    /// `user`, `project` or `external`.
    pub scope: &'static str,
}

/// Discovery roots of one target in precedence-agnostic report order.
#[must_use]
pub fn roots(
    target: &str,
    profile: Option<&HarnessProfile>,
    home: &Path,
    project: Option<&Path>,
) -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(path) = crate::target::skill_root(target, home, project, "user") {
        roots.push(Root {
            path,
            scope: "user",
        });
    }
    if let Some(path) = crate::target::skill_root(target, home, project, "project") {
        roots.push(Root {
            path,
            scope: "project",
        });
    }
    for external in profile.map_or(&[][..], |profile| profile.external_roots) {
        let base = match external.base {
            RootBase::Home => Some(home),
            RootBase::Project => project.filter(|path| !path.as_os_str().is_empty()),
        };
        if let Some(base) = base {
            roots.push(Root {
                path: base.join(external.path),
                scope: "external",
            });
        }
    }
    roots
}

fn skill_dirs(root: &Path, recursive: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    children.sort();
    for child in children {
        // `metadata` follows links: a linked skill directory still counts.
        if !fs::metadata(&child).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        if child.join("SKILL.md").is_file() {
            out.push(child);
        } else if recursive && depth < MAX_DEPTH {
            skill_dirs(&child, recursive, depth + 1, out);
        }
    }
}

/// Finds skill names present more than once across `roots`.
#[must_use]
pub fn find_duplicates(roots: &[Root], recursive: bool) -> Vec<Duplicate> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut by_name: BTreeMap<String, Vec<SkillCopy>> = BTreeMap::new();
    for root in roots {
        let mut dirs = Vec::new();
        skill_dirs(&root.path, recursive, 1, &mut dirs);
        for dir in dirs {
            let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            if !seen.insert(canonical) {
                continue;
            }
            let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            by_name.entry(name.to_owned()).or_default().push(SkillCopy {
                path: dir.to_string_lossy().into_owned(),
                scope: root.scope.to_owned(),
                managed: is_managed_skill(&dir),
            });
        }
    }
    by_name
        .into_iter()
        .filter(|(_, copies)| copies.len() > 1)
        .map(|(name, copies)| Duplicate {
            name,
            copies,
            harness_uses: "unknown".to_owned(),
        })
        .collect()
}
