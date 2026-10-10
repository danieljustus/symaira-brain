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
    body: &[u8],
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
    let yaml = serde_yaml_ng_skill_encoder::to_string(&output)
        .map_err(|error| SkillError(format!("encode frontmatter: {error}")))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"---\n");
    bytes.extend_from_slice(yaml.as_bytes());
    if !yaml.ends_with('\n') {
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"---\n\n");
    bytes.extend_from_slice(body);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::encode_skill_md;
    use crate::model::Frontmatter;
    use serde_json::json;

    #[test]
    fn numeric_json_string_is_quoted_and_ordinary_string_stays_plain() {
        let frontmatter = Frontmatter {
            name: "skills".into(),
            description: "frontmatter style control".into(),
            metadata: json!({"count": "123", "plain": "ordinary text"}),
            ..Frontmatter::default()
        };
        let body = b"\0raw\r\n\xff";
        let encoded = encode_skill_md(&frontmatter, body).expect("encode frontmatter");
        let boundary = encoded
            .windows(b"\n---\n".len())
            .position(|window| window == b"\n---\n")
            .expect("frontmatter closing marker");
        let yaml = std::str::from_utf8(&encoded[4..boundary]).expect("UTF-8 frontmatter");

        // Exact output from the pinned Go yaml.Marshal(Frontmatter) reference.
        let expected_header = b"---\nname: skills\ndescription: frontmatter style control\nmetadata:\n    count: \"123\"\n    plain: ordinary text\n---\n\n";
        assert_eq!(&encoded[..boundary + b"\n---\n\n".len()], expected_header);
        let parsed: Frontmatter = serde_yaml_ng::from_str(yaml).expect("parse generated header");
        assert_eq!(parsed.metadata["count"], json!("123"));
        assert_eq!(parsed.metadata["plain"], json!("ordinary text"));
        assert_eq!(&encoded[boundary + b"\n---\n\n".len()..], body);
    }
}
