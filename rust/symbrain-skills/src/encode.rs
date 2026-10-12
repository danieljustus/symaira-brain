//! Exact SKILL.md frontmatter encoding.

use serde::Serialize;

use crate::model::{Frontmatter, SkillError};

#[path = "encode_metadata.rs"]
mod metadata;

#[cfg(test)]
#[path = "encode_documents_test.rs"]
mod documents_tests;

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
    metadata: metadata::Metadata<'a>,
}

fn str_is_empty(value: &&str) -> bool {
    value.is_empty()
}
fn vec_is_empty(value: &&[String]) -> bool {
    value.is_empty()
}
fn metadata_is_empty(value: &metadata::Metadata<'_>) -> bool {
    value.0.as_object().is_none_or(serde_json::Map::is_empty)
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
        metadata: metadata::Metadata(&frontmatter.metadata),
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

    #[derive(serde::Deserialize)]
    struct ScalarOracleCase {
        id: String,
        context: String,
        #[serde(rename = "type")]
        value_type: String,
        value: String,
        expected_yaml_hex: String,
    }

    fn frontmatter_for_case(case: &ScalarOracleCase) -> Frontmatter {
        use serde_json::{Map, Value};

        let mut frontmatter = Frontmatter {
            name: "probe".into(),
            description: "source extracted".into(),
            ..Frontmatter::default()
        };
        match case.context.as_str() {
            "scalar" => frontmatter.description.clone_from(&case.value),
            "mapping_key" => {
                let mut metadata = Map::new();
                metadata.insert(case.value.clone(), Value::String("marker".into()));
                frontmatter.metadata = Value::Object(metadata);
            }
            "nested_value" => {
                let mut outer = Map::new();
                outer.insert("leaf".into(), Value::String(case.value.clone()));
                let mut metadata = Map::new();
                metadata.insert("outer".into(), Value::Object(outer));
                frontmatter.metadata = Value::Object(metadata);
            }
            "nested_key" => {
                let mut outer = Map::new();
                outer.insert(case.value.clone(), Value::String("marker".into()));
                let mut metadata = Map::new();
                metadata.insert("outer".into(), Value::Object(outer));
                frontmatter.metadata = Value::Object(metadata);
            }
            _ => panic!("unexpected scalar oracle context {:?}", case.context),
        }
        frontmatter
    }

    fn decode_hex(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0, "odd-length oracle hex");
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("ASCII oracle hex");
                u8::from_str_radix(digits, 16).expect("valid oracle hex")
            })
            .collect()
    }

    #[test]
    fn all_740_go_scalar_context_cases_match_shared_encoder() {
        #[derive(serde::Deserialize)]
        struct ApprovedCase {
            id: String,
            expected_yaml_hex: String,
        }
        // Frozen legacy observations stay unchanged. Only these source-captured
        // leading-LF rows use the separately approved value-preserving target.
        let mut overrides: Vec<ApprovedCase> = serde_json::from_str(include_str!(
            "../tests/fixtures/yaml835-leading-lf-target.json"
        ))
        .expect("decode approved leading-LF target");
        assert_eq!(overrides.len(), 12);
        let merge_keys: Vec<ApprovedCase> = serde_json::from_str(include_str!(
            "../tests/fixtures/yaml835-merge-key-target.json"
        ))
        .expect("decode approved merge-key target");
        assert_eq!(merge_keys.len(), 2);
        overrides.extend(merge_keys);
        let mut count = 0;
        let mut changed = 0;
        let mut mismatches = Vec::new();
        let contexts = ["scalar", "mapping_key", "nested_value", "nested_key"];
        for line in include_str!("../tests/fixtures/yaml835-shared-encoder-740.jsonl").lines() {
            let case: ScalarOracleCase = serde_json::from_str(line).expect("decode Go oracle case");
            assert_eq!(case.id, format!("{:03}-{}", count / 4, contexts[count % 4]));
            assert_eq!(case.value_type, "string", "case {}", case.id);
            let frontmatter = frontmatter_for_case(&case);
            let legacy = decode_hex(&case.expected_yaml_hex);
            let expected = if let Some(target) = overrides.iter().find(|row| row.id == case.id) {
                if count / 4 == 177 {
                    assert_eq!(case.value, "<<");
                    assert!(matches!(
                        case.context.as_str(),
                        "mapping_key" | "nested_key"
                    ));
                } else {
                    assert!(matches!(count / 4, 153 | 165 | 166));
                    assert!(case.value.starts_with('\n'));
                }
                let target = decode_hex(&target.expected_yaml_hex);
                assert_ne!(target, legacy, "approved exception must remain explicit");
                changed += 1;
                target
            } else {
                legacy
            };
            let actual = encode_skill_md(&frontmatter, &[]).expect("encode Go oracle case");
            if actual != expected {
                println!("byte mismatch {}: value={:?}", case.id, case.value);
                mismatches.push(case.id.clone());
            }

            let header = std::str::from_utf8(&actual[4..actual.len() - 5])
                .expect("generated header is UTF-8");
            match serde_yaml_ng::from_str::<Frontmatter>(header) {
                Ok(parsed) if parsed == frontmatter => {}
                result => {
                    println!("semantic mismatch {}: {result:?}", case.id);
                    mismatches.push(format!("{}-semantic", case.id));
                }
            }
            count += 1;
        }
        assert_eq!(count, 740);
        assert_eq!(changed, 14);
        println!(
            "executed {count} source-bound scalar cases; {} mismatches",
            mismatches.len()
        );
        assert!(
            mismatches.is_empty(),
            "Go yaml.Marshal mismatches: {mismatches:?}"
        );
    }
}
