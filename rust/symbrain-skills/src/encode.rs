//! Exact SKILL.md frontmatter encoding.

use serde::Serialize;
use serde_json::Value;

use crate::model::{Frontmatter, SkillError};

#[derive(Serialize)]
struct FrontmatterOutput<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(skip_serializing_if = "str_is_empty")]
    category: &'a str,
    #[serde(skip_serializing_if = "str_is_empty")]
    version: &'a str,
    #[serde(skip_serializing_if = "str_is_empty")]
    author: &'a str,
    #[serde(skip_serializing_if = "str_is_empty")]
    license: &'a str,
    #[serde(skip_serializing_if = "str_is_empty")]
    compatibility: &'a str,
    #[serde(skip_serializing_if = "vec_is_empty")]
    platforms: &'a [String],
    #[serde(
        rename = "required_environment_variables",
        skip_serializing_if = "vec_is_empty"
    )]
    required_environment_variables: &'a [String],
    #[serde(skip_serializing_if = "metadata_is_empty")]
    metadata: &'a Value,
}

fn str_is_empty(value: &&str) -> bool {
    value.is_empty()
}
fn vec_is_empty(value: &&[String]) -> bool {
    value.is_empty()
}
fn metadata_is_empty(value: &Value) -> bool {
    value.as_object().is_none_or(serde_json::Map::is_empty)
}

pub(crate) fn encode_skill_md(
    frontmatter: &Frontmatter,
    body: &str,
) -> Result<Vec<u8>, SkillError> {
    let output = FrontmatterOutput {
        name: &frontmatter.name,
        description: &frontmatter.description,
        category: &frontmatter.category,
        version: &frontmatter.version,
        author: &frontmatter.author,
        license: &frontmatter.license,
        compatibility: &frontmatter.compatibility,
        platforms: &frontmatter.platforms,
        required_environment_variables: &frontmatter.required_environment_variables,
        metadata: &frontmatter.metadata,
    };
    let yaml = serde_yaml_ng::to_string(&output)
        .map_err(|error| SkillError(format!("encode frontmatter: {error}")))?;
    let mut in_metadata = false;
    let mut previous_metadata_key_indent = None;
    let yaml = yaml
        .lines()
        .map(|line| {
            if line == "metadata:" {
                in_metadata = true;
                previous_metadata_key_indent = None;
                return line.to_owned();
            }
            if in_metadata && !line.is_empty() {
                let indentation = line.len() - line.trim_start_matches(' ').len();
                let content = &line[indentation..];
                let mut output_indentation = indentation.saturating_mul(2);
                if content.starts_with('-') && previous_metadata_key_indent == Some(indentation) {
                    output_indentation = output_indentation.saturating_add(4);
                }
                previous_metadata_key_indent = content.ends_with(':').then_some(indentation);
                return format!("{}{}", " ".repeat(output_indentation), content);
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("---\n{yaml}\n---\n\n{body}").into_bytes())
}
