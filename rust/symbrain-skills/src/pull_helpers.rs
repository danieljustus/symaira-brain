//! Body, frontmatter and staging helpers behind [`crate::pull`].

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_yaml_ng::{Mapping, Value as Yaml};

use crate::model::{Bundle, SkillError};
use crate::pull::{MANIFEST_FILE, PullChange};
use crate::tree_copy::copy_tree;

/// A parsed `SKILL.md`: raw and decoded frontmatter plus the body.
pub(crate) struct SkillMd {
    pub front: Mapping,
    pub front_text: String,
    pub body: String,
}

pub(crate) fn read_skill_md(path: &Path) -> Result<SkillMd, SkillError> {
    let data =
        fs::read(path).map_err(|error| SkillError(format!("read {}: {error}", path.display())))?;
    let text = String::from_utf8(data)
        .map_err(|_| SkillError(format!("{} is not valid UTF-8", path.display())))?
        .replace("\r\n", "\n");
    let Some(rest) = text.strip_prefix("---\n") else {
        return Err(SkillError(format!(
            "{} has no YAML frontmatter",
            path.display()
        )));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(SkillError(format!(
            "{} frontmatter is not closed",
            path.display()
        )));
    };
    let front_text = rest[..end].to_owned();
    let front = match serde_yaml_ng::from_str::<Yaml>(&front_text)
        .map_err(|error| SkillError(format!("parse {}: {error}", path.display())))?
    {
        Yaml::Mapping(map) => map,
        Yaml::Null => Mapping::new(),
        _ => {
            return Err(SkillError(format!(
                "{} frontmatter is not a mapping",
                path.display()
            )));
        }
    };
    let body = rest[end + 4..].trim_start_matches('\n').to_owned();
    Ok(SkillMd {
        front,
        front_text,
        body,
    })
}

fn overlay_dir(target: &str) -> String {
    format!("overlays/{target}")
}

/// Splits the target's body overlays out of an installed body.
///
/// Returns the portable middle, or the refusals when an overlay region was
/// edited on the harness side.
pub(crate) fn pull_body(
    bundle: &Bundle,
    target: &str,
    installed: &str,
) -> Result<String, Vec<String>> {
    let config = bundle
        .manifest
        .targets
        .get(target)
        .cloned()
        .unwrap_or_default();
    let texts = |name: &str, configured: &str| {
        crate::render::overlay_text(bundle, target, name, configured).map_err(|error| vec![error.0])
    };
    let prepend = texts("prepend.md", &config.prepend)?;
    let append = texts("append.md", &config.append)?;
    let prefix = if prepend.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n\n", prepend.trim_end_matches('\n'))
    };
    let suffix = if append.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n{}", append.trim_end_matches('\n'))
    };
    let installed = installed.trim_start_matches('\n');
    let mut refusals = Vec::new();
    let dir = overlay_dir(target);
    if !prefix.is_empty() && !installed.starts_with(&prefix) {
        refusals.push(format!(
            "body prepend region changed (overlay {dir}/prepend.md)"
        ));
    }
    if !suffix.is_empty() && !installed.trim_end_matches('\n').ends_with(&suffix) {
        refusals.push(format!(
            "body append region changed (overlay {dir}/append.md)"
        ));
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    let mut middle = installed;
    if !prefix.is_empty() {
        middle = middle.strip_prefix(prefix.as_str()).unwrap_or(middle);
    }
    let trimmed;
    if !suffix.is_empty() {
        trimmed = middle.trim_end_matches('\n');
        middle = trimmed.strip_suffix(suffix.as_str()).unwrap_or(trimmed);
    }
    Ok(format!("{}\n", middle.trim_end_matches('\n')))
}

#[derive(Serialize)]
struct StageManifest<'a> {
    target: &'a str,
    name: &'a str,
    changes: &'a [PullChange],
}

/// Writes the pending tree: the library skill plus the pulled edits.
pub(crate) fn stage_tree(
    stage: &Path,
    library: &Path,
    installed: &Path,
    skill_md: &[u8],
    changes: &[PullChange],
    manifest: (&str, &str),
    never_pulled: &dyn Fn(&str) -> bool,
) -> Result<(), SkillError> {
    let io = |error: std::io::Error| SkillError(format!("stage {}: {error}", stage.display()));
    if stage.exists() {
        fs::remove_dir_all(stage).map_err(io)?;
    }
    copy_tree(library, stage, &[MANIFEST_FILE])?;
    fs::write(stage.join("SKILL.md"), skill_md).map_err(io)?;
    for change in changes {
        if never_pulled(&change.path) || change.path == "SKILL.md" {
            continue;
        }
        let destination = stage.join(&change.path);
        if change.status == "removed" {
            match fs::remove_file(&destination) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(io(error));
                }
                _ => continue,
            }
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::copy(installed.join(&change.path), &destination).map_err(io)?;
    }
    let data = serde_json::to_vec_pretty(&StageManifest {
        target: manifest.0,
        name: manifest.1,
        changes,
    })
    .map_err(|error| SkillError(error.to_string()))?;
    let mut data = data;
    data.push(b'\n');
    fs::write(stage.join(MANIFEST_FILE), data).map_err(io)
}
