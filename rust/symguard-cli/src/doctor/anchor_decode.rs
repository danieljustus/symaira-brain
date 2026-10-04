//! Ordered Go-compatible type checks for the auditkit checkpoint shape.
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;

#[derive(Default)]
struct Object(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Object, E> {
                Ok(Object::default())
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Object, M::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(Object(fields))
            }
        }
        decoder.deserialize_any(Fields)
    }
}

/// Syntax is validated separately. `None` preserves unsupported string decoding.
pub(super) fn validate(input: &[u8]) -> Option<Result<(), String>> {
    let first = *input.iter().find(|b| !b.is_ascii_whitespace())?;
    if !matches!(first, b'{' | b'n') {
        return Some(Err(format!(
            "json: cannot unmarshal {} into Go value of type auditkit.ChainAnchor",
            kind(first)
        )));
    }
    let object: Object = serde_json::from_slice(input).ok()?;
    for (field, value) in object.0 {
        let name: String = field
            .chars()
            .map(|c| match c {
                '\u{017f}' => 's',
                '\u{212a}' => 'k',
                _ => c.to_ascii_lowercase(),
            })
            .collect();
        let ty = match name.as_str() {
            "last_entry_hash" | "content_hash" => "string",
            "entry_count" | "log_size" => "int64",
            "schema_version" => "int",
            _ => continue,
        };
        let raw = value.get();
        if raw == "null" {
            continue;
        }
        let first = *raw.as_bytes().first()?;
        let accepted = if ty == "string" && first == b'"' {
            // Unsupported Unicode replacement remains gated, never healthy.
            serde_json::from_str::<String>(raw).ok()?;
            true
        } else if ty == "int" {
            raw.parse::<isize>().is_ok()
        } else {
            ty == "int64" && raw.parse::<i64>().is_ok()
        };
        if !accepted {
            let value_kind = if ty != "string" && matches!(first, b'-' | b'0'..=b'9') {
                format!("number {raw}")
            } else {
                kind(first).to_owned()
            };
            return Some(Err(format!(
                "json: cannot unmarshal {value_kind} into Go struct field ChainAnchor.{name} of type {ty}"
            )));
        }
    }
    Some(Ok(()))
}

fn kind(first: u8) -> &'static str {
    match first {
        b'"' => "string",
        b'[' => "array",
        b'{' => "object",
        b't' | b'f' => "bool",
        _ => "number",
    }
}
