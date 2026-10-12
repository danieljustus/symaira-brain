//! Context-profile inheritance and global/parent/project precedence.
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
    pub path: PathBuf,
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
        let project = if project.is_absolute() {
            project.to_path_buf()
        } else {
            crate::binary::path::join(&std::env::current_dir().unwrap_or_default(), project)
        };
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
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let root = cap_std::fs::Dir::open_ambient_dir(parent, ambient_authority::ambient_authority())
        .map_err(|error| {
        SkillError(format!(
            "read profile {}: {error}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    })?;
    let filename = Path::new(
        path.file_name()
            .ok_or_else(|| SkillError("profile filename missing".into()))?,
    );
    let bytes =
        crate::load::read_limited_nofollow(&root, filename, "profile", crate::MAX_INPUT_SIZE)?;
    let doc: DocumentMut = std::str::from_utf8(&bytes)
        .map_err(|error| SkillError(error.to_string()))?
        .parse()
        .map_err(|error| SkillError(format!("parse profile {}: {error}", filename.display())))?;
    validate_types(&doc)
        .map_err(|error| SkillError(format!("parse profile {}: {error}", filename.display())))?;
    Ok(doc)
}

/// Lists profiles from the most specific context without writing.
/// # Errors
/// Returns bounded inventory, read, and parse failures.
pub fn list(global: &Path, project: Option<&Path>) -> Result<Vec<ProfileRef>, SkillError> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (source, directory) in contexts(global, project) {
        for path in crate::library::library_paths(&directory)? {
            if path.is_dir() || path.extension().is_none_or(|extension| extension != "toml") {
                continue;
            }
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if !seen.insert(name.clone()) {
                continue;
            }
            let doc = load(&path)
                .map_err(|error| SkillError(format!("load profile {}: {error}", path.display())))?;
            result.push(ProfileRef {
                name,
                source: source.clone(),
                path,
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
    // Go resolves most-specific contexts first, then merges in reverse.
    // Keep that error/side-effect order independently of merge precedence.
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
        if row.skill.is_empty() || !library.join(&row.skill).exists() {
            issues.push(Issue {
                code: "profile_missing_skill".into(), severity: "error".into(), path: row.name.clone().into(),
                message: if row.skill.is_empty() { "link has no skill".into() } else { format!("profile {name:?} links skill {:?} which is not in the library; import it with: symskills import <path>", row.skill).into() },
            });
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
            "profile inheritance cycle detected at {name:?}"
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
            let parent = parent
                .as_str()
                .ok_or_else(|| SkillError("inherits entry is not a string".into()))?;
            if parent.trim().is_empty() {
                return Err(SkillError(format!(
                    "profile {declared:?} contains an empty inherits entry"
                )));
            }
            validate_skill_name(parent).map_err(|error| {
                SkillError(format!(
                    "profile {declared:?} inherits invalid name {parent:?}: {error}"
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
                    "profile {declared:?} inherits {parent:?} which was not found"
                ))
            })?;
            for row in resolve_at(contexts, found, parent, visited, count)? {
                links.insert(row.name.clone(), row);
            }
        }
    }
    if let Some(table) = doc.get("links").and_then(toml_edit::Item::as_table_like) {
        for (name, link) in table.iter() {
            if links.len() >= crate::MAX_RESOURCE_ENTRIES {
                return Err(SkillError("profile links exceed entry limit".into()));
            }
            links.insert(
                name.into(),
                Resolved {
                    name: name.into(),
                    skill: link
                        .get("skill")
                        .and_then(toml_edit::Item::as_str)
                        .unwrap_or_default()
                        .into(),
                    alias: link
                        .get("alias")
                        .and_then(toml_edit::Item::as_str)
                        .unwrap_or_default()
                        .into(),
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
        Err(error) => Err(SkillError(format!("{context} {identity}: {error}"))),
    }
}
