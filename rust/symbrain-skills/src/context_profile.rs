//! Context-profile listing and inheritance (`skills_profile_list` / `skills_profile_resolve`).
//!
//! The MCP gateway passes no project directory, so only the global profile
//! context is reachable from the tools; project/parent contexts are kept for
//! the shared Go precedence rules.

use crate::{Issue, SkillError, validate_skill_name};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

/// One discovered context profile.
#[derive(Debug, Serialize)]
pub struct ProfileRef {
    /// Filename stem.
    pub name: String,
    /// Winning context.
    pub source: String,
    /// Profile source path.
    pub path: String,
    /// Optional description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// One merged skill link.
#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    /// Link name in the context profile.
    pub name: String,
    /// Library skill identity.
    pub skill: String,
    /// Optional install alias.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alias: String,
    /// Context owning the link.
    pub source: String,
    /// Declared profile name.
    pub profile: String,
}

fn contexts(global: &Path, project: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    if let Some(project) = project.filter(|path| !path.as_os_str().is_empty()) {
        let project = crate::gotext::absolute(project);
        result.push((
            "project".into(),
            project.join(".symskills").join("profiles"),
        ));
        for (index, parent) in project.ancestors().skip(1).enumerate() {
            result.push((
                format!("parent:{}", index + 1),
                parent.join(".symskills").join("profiles"),
            ));
        }
    }
    if !global.as_os_str().is_empty() {
        result.push(("global".into(), global.to_path_buf()));
    }
    result
}

fn load(path: &Path) -> Result<DocumentMut, SkillError> {
    let base = path.file_name().unwrap_or_default().to_string_lossy();
    let bytes = read_bounded(path).map_err(|error| {
        SkillError(format!(
            "read profile {base}: {}",
            crate::gotext::io_text(&error)
        ))
    })?;
    let doc: DocumentMut = std::str::from_utf8(&bytes)
        .map_err(|error| SkillError(format!("parse profile {base}: {error}")))?
        .parse()
        .map_err(|error| SkillError(format!("parse profile {base}: {error}")))?;
    validate_types(&doc).map_err(|error| SkillError(format!("parse profile {base}: {error}")))?;
    Ok(doc)
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(crate::MAX_INPUT_SIZE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > crate::MAX_INPUT_SIZE {
        return Err(std::io::Error::other("profile exceeds skills input limit"));
    }
    Ok(bytes)
}

/// Lists profiles from the most specific context without writing.
///
/// # Errors
/// Returns bounded inventory, read, and parse failures.
pub fn list(global: &Path, project: Option<&Path>) -> Result<Vec<ProfileRef>, SkillError> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (source, directory) in contexts(global, project) {
        let entries = match crate::library::read_library_entries(&directory) {
            Ok(entries) => entries,
            Err(crate::library::LibraryReadError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(crate::library::LibraryReadError::Io(error)) => {
                return Err(SkillError(format!(
                    "list profiles {}: {}",
                    directory.display(),
                    crate::gotext::path_error("open", &directory, &error)
                )));
            }
            Err(error) => {
                return Err(SkillError(format!(
                    "list profiles {}: {error}",
                    directory.display()
                )));
            }
        };
        for entry in entries {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let Some(name) = file_name.strip_suffix(".toml") else {
                continue;
            };
            if is_dir {
                continue;
            }
            if !seen.insert(name.to_owned()) {
                continue;
            }
            let path = entry.path();
            let doc = load(&path)
                .map_err(|error| SkillError(format!("load profile {}: {error}", path.display())))?;
            result.push(ProfileRef {
                name: name.to_owned(),
                source: source.clone(),
                path: path.to_string_lossy().into_owned(),
                description: doc
                    .get("description")
                    .and_then(toml_edit::Item::as_str)
                    .unwrap_or_default()
                    .into(),
            });
        }
    }
    Ok(result)
}

/// Resolves inheritance in each context, then merges least to most specific.
///
/// # Errors
/// Rejects invalid names, inheritance cycles and unreadable profiles.
pub fn resolve(
    library: &Path,
    global: &Path,
    project: Option<&Path>,
    name: &str,
) -> Result<(Vec<Resolved>, Vec<Issue>), SkillError> {
    validate_skill_name(name)
        .map_err(|error| SkillError(format!("invalid profile name: {error}")))?;
    let contexts = contexts(global, project);
    if contexts.is_empty() {
        return Err(SkillError("no profile contexts configured".into()));
    }
    // Go resolves most-specific contexts first, then merges in reverse. Keep
    // that error order independently of merge precedence.
    let mut resolved = Vec::new();
    let mut count = 0;
    for index in 0..contexts.len() {
        let path = contexts[index].1.join(format!("{name}.toml"));
        if present(&path, "stat profile", &path.display().to_string())? {
            resolved.push(resolve_at(
                &contexts,
                index,
                name,
                &mut BTreeSet::new(),
                &mut count,
            )?);
        }
    }
    let mut merged = BTreeMap::new();
    for rows in resolved.into_iter().rev() {
        for row in rows {
            merged.insert(row.name.clone(), row);
        }
    }
    let rows: Vec<Resolved> = merged.into_values().collect();
    let mut issues = Vec::new();
    for row in &rows {
        if row.skill.is_empty() {
            issues.push(Issue {
                code: "profile_missing_skill".into(),
                severity: "error".into(),
                message: "link has no skill".into(),
                path: row.name.clone(),
            });
        } else if let Err(error) = std::fs::metadata(library.join(&row.skill)) {
            if error.kind() == std::io::ErrorKind::NotFound {
                issues.push(Issue {
                    code: "profile_missing_skill".into(),
                    severity: "error".into(),
                    message: format!(
                        "profile {} links skill {} which is not in the library; import it with: symskills import <path>",
                        crate::gotext::quote(name),
                        crate::gotext::quote(&row.skill)
                    ),
                    path: row.name.clone(),
                });
            } else {
                issues.push(Issue {
                    code: "profile_library_read".into(),
                    severity: "error".into(),
                    message: crate::gotext::path_error("stat", &library.join(&row.skill), &error),
                    path: row.skill.clone(),
                });
            }
        }
    }
    Ok((rows, issues))
}

fn resolve_at(
    contexts: &[(String, PathBuf)],
    index: usize,
    name: &str,
    visited: &mut BTreeSet<String>,
    count: &mut usize,
) -> Result<Vec<Resolved>, SkillError> {
    if !visited.insert(name.into()) {
        return Err(SkillError(format!(
            "profile inheritance cycle detected at {}",
            crate::gotext::quote(name)
        )));
    }
    *count += 1;
    if *count > crate::MAX_RESOURCE_ENTRIES || visited.len() > crate::MAX_RESOURCE_DEPTH {
        return Err(SkillError("profile inheritance exceeds input limit".into()));
    }
    let doc = load(&contexts[index].1.join(format!("{name}.toml")))?;
    let declared = doc
        .get("name")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or_default();
    let mut links = BTreeMap::new();
    if let Some(parents) = doc.get("inherits").and_then(toml_edit::Item::as_array) {
        for parent in parents {
            let parent = parent.as_str().unwrap_or_default();
            if parent.trim().is_empty() {
                return Err(SkillError(format!(
                    "profile {} contains an empty inherits entry",
                    crate::gotext::quote(declared)
                )));
            }
            validate_skill_name(parent).map_err(|error| {
                SkillError(format!(
                    "profile {} inherits invalid name {}: {error}",
                    crate::gotext::quote(declared),
                    crate::gotext::quote(parent)
                ))
            })?;
            let mut found = None;
            for (next, (_, directory)) in contexts.iter().enumerate().skip(index) {
                let path = directory.join(format!("{parent}.toml"));
                if present(&path, "stat inherited profile", parent)? {
                    found = Some(next);
                    break;
                }
            }
            let found = found.ok_or_else(|| {
                SkillError(format!(
                    "profile {} inherits {} which was not found",
                    crate::gotext::quote(declared),
                    crate::gotext::quote(parent)
                ))
            })?;
            for row in resolve_at(contexts, found, parent, visited, count)? {
                links.insert(row.name.clone(), row);
            }
        }
    }
    if let Some(table) = doc.get("links").and_then(toml_edit::Item::as_table_like) {
        for (link_name, link) in table.iter() {
            if links.len() >= crate::MAX_RESOURCE_ENTRIES {
                return Err(SkillError("profile links exceed entry limit".into()));
            }
            let field = |key: &str| {
                link.get(key)
                    .and_then(toml_edit::Item::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            links.insert(
                link_name.into(),
                Resolved {
                    name: link_name.into(),
                    skill: field("skill"),
                    alias: field("alias"),
                    source: contexts[index].0.clone(),
                    profile: declared.into(),
                },
            );
        }
    }
    visited.remove(name);
    Ok(links.into_values().collect())
}

fn validate_types(doc: &DocumentMut) -> Result<(), SkillError> {
    for key in ["name", "description"] {
        if doc.get(key).is_some_and(|item| item.as_str().is_none()) {
            return Err(SkillError(format!("field {key} is not a string")));
        }
    }
    if let Some(item) = doc.get("inherits") {
        let values = item
            .as_array()
            .ok_or_else(|| SkillError("inherits is not an array of strings".into()))?;
        if values.iter().any(|item| item.as_str().is_none()) {
            return Err(SkillError("inherits is not an array of strings".into()));
        }
    }
    if let Some(item) = doc.get("links") {
        let links = item
            .as_table_like()
            .ok_or_else(|| SkillError("links is not a table".into()))?;
        for (name, item) in links.iter() {
            let link = item
                .as_table_like()
                .ok_or_else(|| SkillError(format!("links.{name} is not a table")))?;
            for key in ["skill", "alias"] {
                if link.get(key).is_some_and(|item| item.as_str().is_none()) {
                    return Err(SkillError(format!("links.{name}.{key} is not a string")));
                }
            }
        }
    }
    Ok(())
}

fn present(path: &Path, context: &str, identity: &str) -> Result<bool, SkillError> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SkillError(format!(
            "{context} {identity}: {}",
            crate::gotext::io_text(&error)
        ))),
    }
}
