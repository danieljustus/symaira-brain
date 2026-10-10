//! Cumulative actual-Go document corpus, not scalar-only evidence.
use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::encode_skill_md;
use crate::model::Frontmatter;

#[derive(Deserialize)]
struct Input {
    id: String,
    frontmatter: Frontmatter,
    body_hex: String,
}

#[derive(Deserialize)]
struct Expected {
    id: String,
    yaml_hex: String,
    body_hex: String,
    frontmatter: Frontmatter,
    parse_error: String,
    input_number_types: BTreeMap<String, String>,
    parsed_number_types: BTreeMap<String, String>,
}

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn number_types(value: &Value, path: &str, result: &mut BTreeMap<String, String>) {
    match value {
        Value::Number(number) => {
            let kind = if number.is_i64() {
                "i64".into()
            } else if number.is_u64() {
                "u64".into()
            } else {
                format!("f64:{:016x}", number.as_f64().unwrap().to_bits())
            };
            result.insert(path.into(), kind);
        }
        Value::Object(map) => {
            for (key, item) in map {
                let key = key.replace('~', "~0").replace('/', "~1");
                number_types(item, &format!("{path}/{key}"), result);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                number_types(item, &format!("{path}/{index}"), result);
            }
        }
        _ => {}
    }
}

fn check(input_bytes: &str, expected_bytes: &str) -> Result<usize, String> {
    let inputs: Vec<Input> = input_bytes
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let expected: Vec<Expected> = expected_bytes
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    if inputs.len() != 38 || expected.len() != 38 {
        return Err("case inventory".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for (input, expected) in inputs.iter().zip(&expected) {
        let id = &input.id;
        if id != &expected.id || !ids.insert(id) {
            return Err(format!("case ID {id}"));
        }
        let mut types = BTreeMap::new();
        number_types(&input.frontmatter.metadata, "/metadata", &mut types);
        if types != expected.input_number_types {
            return Err(format!(
                "input runtime types/bits {id}: {types:?} != {:?}",
                expected.input_number_types
            ));
        }
        let body = hex(&input.body_hex);
        if body != hex(&expected.body_hex) {
            return Err(format!("body input {id}"));
        }
        let bytes = encode_skill_md(&input.frontmatter, &body).unwrap();
        if bytes != hex(&expected.yaml_hex) {
            return Err(format!(
                "document bytes {id}: {}",
                String::from_utf8_lossy(&bytes)
            ));
        }
        if !expected.parse_error.is_empty() {
            return Err(format!("Go reparse {id}"));
        }
        // Remove only the known framing lengths; the body can itself contain delimiters.
        let header_end = bytes.len() - body.len() - b"---\n\n".len();
        let header = std::str::from_utf8(&bytes[4..header_end]).unwrap();
        let parsed: Frontmatter = serde_yaml_ng::from_str(header).unwrap();
        // JSON cannot distinguish Go's marshaled integral float from an integer.
        // Restore the captured runtime bits before comparing the typed model.
        let mut expected_frontmatter = expected.frontmatter.clone();
        for (path, kind) in &expected.parsed_number_types {
            if let Some(bits) = kind.strip_prefix("f64:") {
                let bits = u64::from_str_radix(bits, 16).unwrap();
                *expected_frontmatter
                    .metadata
                    .pointer_mut(path.strip_prefix("/metadata").unwrap())
                    .unwrap() =
                    Value::Number(serde_json::Number::from_f64(f64::from_bits(bits)).unwrap());
            }
        }
        if parsed != expected_frontmatter {
            return Err(format!("parsed type/value {id}: {parsed:?}"));
        }
        types.clear();
        number_types(&parsed.metadata, "/metadata", &mut types);
        if types != expected.parsed_number_types {
            return Err(format!("parsed runtime types/bits {id}: {types:?}"));
        }
        if bytes[header_end + 5..] != body {
            return Err(format!("opaque body {id}"));
        }
    }
    Ok(inputs.len())
}

#[test]
fn actual_go_38_typed_control_documents_and_changed_expectation_rejection() {
    let input = include_str!("../tests/fixtures/yaml835-documents-input.jsonl");
    let expected = include_str!("../tests/fixtures/yaml835-documents-go.jsonl");
    assert_eq!(check(input, expected), Ok(38));
    println!("executed 28 original + 10 cumulative documents, exact bytes/types/bits/body");
    let mut rows: Vec<Value> = expected
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows[0]["yaml_hex"] = Value::String("00".into());
    let mutation = rows
        .iter()
        .map(|row| serde_json::to_string(row).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        check(input, &mutation)
            .unwrap_err()
            .starts_with("document bytes body-empty")
    );
}
