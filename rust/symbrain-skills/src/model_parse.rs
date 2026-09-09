//! Frontmatter and manifest parsing for skill bundles.

use toml_edit::{DocumentMut, Item};

use super::{
    BTreeMap, Frontmatter, Manifest, ParsedSkill, SkillError, TargetConfig, normalize_category,
};

/// Parses a UTF-8 SKILL.md, normalizing CRLF as the Go implementation does.
/// The returned body retains all content and trailing bytes after the closing fence.
///
/// # Errors
///
/// Returns an error when the bytes are not UTF-8 or the frontmatter is malformed.
pub fn parse_skill_md(raw: &[u8]) -> Result<ParsedSkill, SkillError> {
    let text = String::from_utf8(raw.to_vec())
        .map_err(|error| SkillError(format!("parse SKILL.md as UTF-8: {error}")))?
        .replace("\r\n", "\n");
    if !text.starts_with("---\n") {
        return Err(SkillError(
            "SKILL.md must start with YAML frontmatter".into(),
        ));
    }
    let rest = &text[4..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| SkillError("SKILL.md frontmatter is not closed".into()))?;
    let header = &rest[..end];
    if header.len() > crate::model::MAX_FRONTMATTER_SIZE {
        return Err(SkillError(format!(
            "SKILL.md frontmatter exceeds maximum size of {} bytes",
            crate::model::MAX_FRONTMATTER_SIZE
        )));
    }
    let mut body_start = 4 + end + 4;
    while text.as_bytes().get(body_start) == Some(&b'\n') {
        body_start += 1;
    }
    let body = text[body_start..].to_string();
    let frontmatter = parse_frontmatter(header)?;
    Ok(ParsedSkill {
        frontmatter,
        body,
        body_line_offset: text[..body_start].matches('\n').count(),
    })
}

fn parse_frontmatter(text: &str) -> Result<Frontmatter, SkillError> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)
        .map_err(|error| SkillError(format!("parse SKILL.md frontmatter: {error}")))?;
    let map = value
        .as_mapping()
        .ok_or_else(|| SkillError("frontmatter must be a mapping".into()))?;
    let string = |name: &str| -> Result<String, SkillError> {
        let Some(value) = map.get(serde_yaml_ng::Value::String(name.into())) else {
            return Ok(String::new());
        };
        if value.is_null() {
            return Ok(String::new());
        }
        if let Some(scalar) = yaml_scalar_string(value) {
            return Ok(scalar);
        }
        if let Some(values) = value.as_sequence() {
            let mut strings = Vec::with_capacity(values.len());
            for value in values {
                strings.push(yaml_scalar_string(value).ok_or_else(|| {
                    SkillError(format!(
                        "frontmatter field {name:?} must be a string or a list of strings"
                    ))
                })?);
            }
            return Ok(strings.join(", "));
        }
        Err(SkillError(format!(
            "frontmatter field {name:?} must be a string or a list of strings"
        )))
    };
    let strings = |name: &str| -> Result<Vec<String>, SkillError> {
        let Some(value) = map.get(serde_yaml_ng::Value::String(name.into())) else {
            return Ok(Vec::new());
        };
        if value.is_null() {
            return Ok(Vec::new());
        }
        value
            .as_sequence()
            .ok_or_else(|| {
                SkillError(format!(
                    "frontmatter field {name:?} must be a list of strings"
                ))
            })?
            .iter()
            .map(|value| {
                yaml_scalar_string(value).ok_or_else(|| {
                    SkillError(format!(
                        "frontmatter field {name:?} must be a string or a list of strings"
                    ))
                })
            })
            .collect()
    };
    let metadata = map
        .get(serde_yaml_ng::Value::String("metadata".into()))
        .map_or(
            Ok(serde_json::Value::Object(serde_json::Map::new())),
            |value| {
                if value.is_null() {
                    Ok(serde_json::Value::Object(serde_json::Map::new()))
                } else {
                    serde_json::to_value(value).map_err(|error| {
                        SkillError(format!("frontmatter field \"metadata\": {error}"))
                    })
                }
            },
        )?;
    Ok(Frontmatter {
        name: string("name")?,
        description: string("description")?,
        category: normalize_category(&string("category")?),
        version: string("version")?,
        author: string("author")?,
        license: string("license")?,
        compatibility: string("compatibility")?,
        platforms: strings("platforms")?,
        required_environment_variables: strings("required_environment_variables")?,
        metadata,
    })
}

fn yaml_scalar_string(value: &serde_yaml_ng::Value) -> Option<String> {
    match value {
        serde_yaml_ng::Value::String(value) => Some(value.clone()),
        serde_yaml_ng::Value::Bool(value) => Some(value.to_string()),
        serde_yaml_ng::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn parse_manifest(text: &str) -> Result<Manifest, SkillError> {
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|error| SkillError(format!("parse symskills.toml: {error}")))?;
    let mut manifest = Manifest {
        targets: BTreeMap::new(),
        ..Manifest::default()
    };
    if let Some(item) = doc.get("skill") {
        let table = item
            .as_table_like()
            .ok_or_else(|| SkillError("expected a table for skill".into()))?;
        manifest.skill.name = str_field(table.get("name"), "skill.name")?;
        manifest.skill.version = str_field(table.get("version"), "skill.version")?;
        manifest.skill.source = str_field(table.get("source"), "skill.source")?;
        manifest.skill.allow_executable =
            bool_field(table.get("allow_executable"), "skill.allow_executable")?.unwrap_or(false);
        manifest.skill.requires = array_field(table.get("requires"), "skill.requires")?;
    }
    if let Some(item) = doc.get("targets") {
        let table = item
            .as_table_like()
            .ok_or_else(|| SkillError("expected a table for targets".into()))?;
        for (name, item) in table.iter() {
            let target = item
                .as_table_like()
                .ok_or_else(|| SkillError(format!("expected a table for targets.{name}")))?;
            manifest.targets.insert(
                name.to_string(),
                TargetConfig {
                    enabled: bool_field(target.get("enabled"), &format!("targets.{name}.enabled"))?
                        .unwrap_or(false),
                    alias: str_field(target.get("alias"), &format!("targets.{name}.alias"))?,
                    description: str_field(
                        target.get("description"),
                        &format!("targets.{name}.description"),
                    )?,
                    scope: str_field(target.get("scope"), &format!("targets.{name}.scope"))?,
                    category: str_field(
                        target.get("category"),
                        &format!("targets.{name}.category"),
                    )?,
                    prepend: str_field(target.get("prepend"), &format!("targets.{name}.prepend"))?,
                    append: str_field(target.get("append"), &format!("targets.{name}.append"))?,
                    metadata: string_table(
                        target.get("metadata"),
                        &format!("targets.{name}.metadata"),
                    )?,
                },
            );
        }
    }
    if let Some(item) = doc.get("terms") {
        let table = item
            .as_table_like()
            .ok_or_else(|| SkillError("expected a table for terms".into()))?;
        for (name, item) in table.iter() {
            let values = item
                .as_table_like()
                .ok_or_else(|| SkillError(format!("expected a table for terms.{name}")))?;
            let mut terms = BTreeMap::new();
            for (key, value) in values.iter() {
                terms.insert(
                    key.to_string(),
                    str_field(Some(value), &format!("terms.{name}.{key}"))?,
                );
            }
            manifest.terms.insert(name.to_string(), terms);
        }
    }
    Ok(manifest)
}

fn str_field(item: Option<&Item>, name: &str) -> Result<String, SkillError> {
    item.map_or(Ok(String::new()), |item| {
        item.as_str()
            .map(ToString::to_string)
            .ok_or_else(|| SkillError(format!("expected a string for {name}")))
    })
}
fn bool_field(item: Option<&Item>, name: &str) -> Result<Option<bool>, SkillError> {
    item.map_or(Ok(None), |item| {
        item.as_bool()
            .map(Some)
            .ok_or_else(|| SkillError(format!("expected a boolean for {name}")))
    })
}
fn array_field(item: Option<&Item>, name: &str) -> Result<Vec<String>, SkillError> {
    item.map_or(Ok(Vec::new()), |item| {
        item.as_array()
            .ok_or_else(|| SkillError(format!("expected an array for {name}")))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToString::to_string)
                    .ok_or_else(|| SkillError(format!("expected string element in {name} array")))
            })
            .collect()
    })
}
fn string_table(item: Option<&Item>, name: &str) -> Result<BTreeMap<String, String>, SkillError> {
    item.map_or(Ok(BTreeMap::new()), |item| {
        let table = item
            .as_table_like()
            .ok_or_else(|| SkillError(format!("expected a table for {name}")))?;
        table
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.to_string(),
                    str_field(Some(value), &format!("{name}.{key}"))?,
                ))
            })
            .collect()
    })
}
