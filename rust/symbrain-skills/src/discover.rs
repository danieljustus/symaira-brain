//! Read-only unmanaged skill discovery with bounded loader and inventory.
use crate::{BundleLoader, SkillError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Candidate inventory shape from the shipped discovery API.
#[derive(Debug, Serialize)]
pub struct Candidate {
    /// Stable truncated path-and-content hash.
    pub source_id: String,
    /// Known harness target, absent for explicit paths.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// Always skill_bundle.
    pub kind: &'static str,
    /// Frontmatter name when loadable, otherwise path basename.
    pub display_name: String,
    /// Observed source path.
    pub location: PathBuf,
    /// Existing managed marker.
    pub managed: bool,
    /// All validation errors absent.
    pub valid: bool,
    /// harness-root:<target> or explicit-path.
    pub source: String,
    /// candidate/managed/invalid/unreadable.
    pub status: &'static str,
    /// Optional source/validation diagnostics.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}
fn candidate(path: &Path, source: &str, target: &str, loader: &BundleLoader) -> Candidate {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let loaded = loader.load(path);
    // Identity includes raw SKILL.md even when frontmatter fails to load.
    // Retain the same bounded, no-follow reader rather than weakening the
    // library loader merely to obtain an invalid candidate's identifier.
    let bytes = cap_std::fs::Dir::open_ambient_dir(path, ambient_authority::ambient_authority())
        .ok()
        .and_then(|root| {
            crate::load::read_limited_nofollow(
                &root,
                Path::new("SKILL.md"),
                "discovered skill",
                crate::MAX_INPUT_SIZE,
            )
            .ok()
        })
        .unwrap_or_default();
    let mut hash = Sha256::new();
    // Go hashes native Unix bytes / Windows UTF16ToString WTF-8 before
    // JSON encoding repairs invalid bytes for presentation.
    hash.update(canonical.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(bytes);
    let managed =
        path.join(".symskills.json").exists() || canonical.join(".symskills.json").exists();
    let mut row = Candidate {
        source_id: format!("sha256:{:x}", hash.finalize())[..23].into(),
        target: target.into(),
        kind: "skill_bundle",
        display_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        location: path.to_path_buf(),
        managed,
        valid: true,
        source: source.into(),
        status: if managed { "managed" } else { "candidate" },
        diagnostics: Vec::new(),
    };
    match loaded {
        Err(error) => {
            row.valid = false;
            row.status = "invalid";
            row.diagnostics.push(error.to_string());
        }
        Ok(bundle) => {
            if !bundle.frontmatter.name.is_empty() {
                row.display_name = bundle.frontmatter.name.clone();
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
/// Discovers harness roots plus explicit paths without copying/writing skills.
/// # Errors
/// Returns bounds failures; ordinary inaccessible explicit paths are rows.
pub fn scanned(
    home: &Path,
    project: Option<&Path>,
    scope: &str,
    paths: &[String],
) -> Result<Vec<Candidate>, SkillError> {
    if paths.len() > crate::MAX_RESOURCE_ENTRIES {
        return Err(SkillError("discovery paths exceed entry limit".into()));
    }
    let mut roots: Vec<_> = crate::default_targets()
        .into_iter()
        .filter_map(|target| {
            crate::skill_root(&target, home, project, scope)
                .map(|path| (path, format!("harness-root:{target}"), target))
        })
        .collect();
    for path in paths.iter().filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            crate::binary::path::join(&std::env::current_dir().unwrap_or_default(), &path)
        };
        roots.push((path, "explicit-path".into(), String::new()));
    }
    let loader = BundleLoader::default();
    let mut found = BTreeMap::new();
    for (path, source, target) in roots {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if source == "explicit-path" {
                    let mut row = candidate(&path, &source, &target, &loader);
                    row.status = "unreadable";
                    row.valid = false;
                    row.managed = false;
                    #[cfg(windows)]
                    let operation = if error.kind() == std::io::ErrorKind::NotFound {
                        "GetFileAttributesEx"
                    } else {
                        "CreateFile"
                    };
                    #[cfg(not(windows))]
                    let operation = "lstat";
                    row.diagnostics = vec![format!(
                        "path not accessible: {}",
                        crate::io_contract::path_error(operation, &path, &error)
                    )];
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
            Err(error) if source == "explicit-path" => {
                let mut row = candidate(&path, &source, &target, &loader);
                row.status = "unreadable";
                row.valid = false;
                row.managed = false;
                let diagnostic = match error {
                    crate::library::LibraryReadError::Io(error) => {
                        crate::io_contract::path_error("open", &path, &error)
                    }
                    crate::library::LibraryReadError::InputBound => error.to_string(),
                };
                row.diagnostics = vec![format!("read directory: {diagnostic}")];
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
