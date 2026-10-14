//! Exact SKILL.md frontmatter encoding.

use serde::ser::{Error as _, SerializeMap as _, SerializeSeq as _};
use serde::{Serialize, Serializer};
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
    metadata: Metadata<'a>,
}

struct Metadata<'a>(&'a Value);

impl Serialize for Metadata<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values {
                    map.serialize_entry(key, &Metadata(value))?;
                }
                map.end()
            }
            Value::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(&Metadata(value))?;
                }
                sequence.end()
            }
            Value::Number(number) => {
                // CLI feature unification enables JSON's private number struct.
                // Keep YAML numbers numeric, as in the scoped #857 correction.
                if let Some(value) = number.as_i64() {
                    serializer.serialize_i64(value)
                } else if let Some(value) = number.as_u64() {
                    serializer.serialize_u64(value)
                } else {
                    let value = number
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .ok_or_else(|| {
                            S::Error::custom("metadata number is outside finite f64 range")
                        })?;
                    serializer.serialize_f64(value)
                }
            }
            value => value.serialize(serializer),
        }
    }
}

fn str_is_empty(value: &&str) -> bool {
    value.is_empty()
}
fn vec_is_empty(value: &&[String]) -> bool {
    value.is_empty()
}
fn metadata_is_empty(value: &Metadata<'_>) -> bool {
    value.0.as_object().is_none_or(serde_json::Map::is_empty)
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
        metadata: Metadata(&frontmatter.metadata),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_serializes_numeric_primitives_through_nested_collections() {
        let metadata = serde_json::json!({
            "nested": [i64::MIN, u64::MAX, -0.0, 2.5, true, "2", null]
        });
        let yaml = serde_yaml_ng::to_string(&Metadata(&metadata)).expect("numeric metadata");
        assert!(!yaml.contains("$serde_json::private::Number"));
        let reparsed: Value = serde_yaml_ng::from_str(&yaml).expect("typed metadata");
        assert_eq!(reparsed, metadata);
        assert_eq!(
            reparsed["nested"][2]
                .as_f64()
                .expect("negative zero")
                .to_bits(),
            (-0.0_f64).to_bits()
        );
    }
}
