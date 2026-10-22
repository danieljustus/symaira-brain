//! Read-only discovery of unmanaged skill sources (`skills_discover_sources`).

use crate::{BundleLoader, SkillError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One discovered skill bundle candidate.
#[derive(Debug, Serialize)]
pub struct Candidate {
    /// Stable truncated path-and-content hash.
    pub source_id: String,
    /// Known harness target, absent for explicit paths.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// Always `skill_bundle`.
    pub kind: &'static str,
    /// Frontmatter name when loadable, otherwise path basename.
    pub display_name: String,
    /// Observed source path.
    pub location: String,
    /// Existing managed marker.
    pub managed: bool,
    /// All validation errors absent.
    pub valid: bool,
    /// `harness-root:<target>` or `explicit-path`.
    pub source: String,
    /// `candidate`, `managed`, `invalid` or `unreadable`.
    pub status: &'static str,
    /// Optional source/validation diagnostics.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

fn source_id(path: &Path) -> String {
    let content = std::fs::read(path.join("SKILL.md")).unwrap_or_default();
    let mut hash = Sha256::new();
    hash.update(path.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(content);
    format!("sha256:{:x}", hash.finalize())[..23].into()
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn candidate(path: &Path, source: &str, target: &str, loader: &BundleLoader) -> Candidate {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let managed =
        path.join(".symskills.json").exists() || canonical.join(".symskills.json").exists();
    let mut row = Candidate {
        source_id: source_id(&canonical),
        target: target.into(),
        kind: "skill_bundle",
        display_name: base_name(path),
        location: path.to_string_lossy().into_owned(),
        managed,
        valid: true,
        source: source.into(),
        status: if managed { "managed" } else { "candidate" },
        diagnostics: Vec::new(),
    };
    match loader.load(path) {
        Err(error) => {
            row.valid = false;
            row.status = "invalid";
            row.diagnostics.push(error.to_string());
        }
        Ok(bundle) => {
            if !bundle.frontmatter.name.is_empty() {
                row.display_name.clone_from(&bundle.frontmatter.name);
            }
            for issue in crate::validate(&bundle) {
                row.diagnostics.push(format!(
                    "[{}] {}: {}",
                    issue.severity, issue.code, issue.message
                ));
                if issue.severity == "error" {
                    row.valid = false;
                    if row.status == "candidate" {
                        row.status = "invalid";
                    }
                }
            }
        }
    }
    row
}

fn unreadable(path: &Path, source: &str, target: &str, diagnostic: String) -> Candidate {
    Candidate {
        source_id: source_id(path),
        target: target.into(),
        kind: "skill_bundle",
        display_name: base_name(path),
        location: path.to_string_lossy().into_owned(),
        managed: false,
        valid: false,
        source: source.into(),
        status: "unreadable",
        diagnostics: vec![diagnostic],
    }
}

/// Discovers harness roots plus explicit paths without copying/writing skills.
///
/// The MCP gateway has no project directory, so `project` roots fall back to
/// the user roots exactly like Go's registry does for an empty project path.
///
/// # Errors
/// Returns bounds failures; ordinary inaccessible explicit paths are rows.
pub fn scanned(home: &Path, paths: &[String]) -> Result<Vec<Candidate>, SkillError> {
    if paths.len() > crate::MAX_RESOURCE_ENTRIES {
        return Err(SkillError("discovery paths exceed entry limit".into()));
    }
    let mut roots: Vec<(PathBuf, String, String)> = crate::default_targets()
        .into_iter()
        .filter_map(|target| {
            crate::skill_root(&target, home, None, "user")
                .map(|path| (path, format!("harness-root:{target}"), target))
        })
        .collect();
    for path in paths.iter().filter(|path| !path.is_empty()) {
        roots.push((
            crate::gotext::absolute(Path::new(path)),
            "explicit-path".into(),
            String::new(),
        ));
    }
    let loader = BundleLoader::default();
    let mut found: BTreeMap<String, Candidate> = BTreeMap::new();
    for (path, source, target) in roots {
        let explicit = source == "explicit-path";
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if explicit {
                    let row = unreadable(
                        &path,
                        &source,
                        &target,
                        format!(
                            "path not accessible: {}",
                            crate::gotext::path_error("lstat", &path, &error)
                        ),
                    );
                    found.insert(row.source_id.clone(), row);
                }
                continue;
            }
        };
        if path.join("SKILL.md").exists() {
            let row = candidate(&path, &source, &target, &loader);
            found.insert(row.source_id.clone(), row);
            continue;
        }
        if !metadata.is_dir() && !metadata.file_type().is_symlink() {
            continue;
        }
        match crate::library::read_library_entries(&path) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.path();
                    if !entry.join("SKILL.md").exists() {
                        continue;
                    }
                    let row = candidate(&entry, &source, &target, &loader);
                    match found.get(&row.source_id) {
                        Some(previous) if !previous.target.is_empty() || row.target.is_empty() => {}
                        _ => {
                            found.insert(row.source_id.clone(), row);
                        }
                    }
                }
            }
            Err(error) if explicit => {
                let text = match error {
                    crate::library::LibraryReadError::Io(error) => {
                        crate::gotext::path_error("open", &path, &error)
                    }
                    crate::library::LibraryReadError::InputBound => error.to_string(),
                };
                let row = unreadable(&path, &source, &target, format!("read directory: {text}"));
                found.insert(row.source_id.clone(), row);
            }
            Err(_) => {}
        }
    }
    let mut rows: Vec<_> = found.into_values().collect();
    rows.sort_by(|a, b| {
        a.location
            .cmp(&b.location)
            .then(a.source_id.cmp(&b.source_id))
    });
    Ok(rows)
}
