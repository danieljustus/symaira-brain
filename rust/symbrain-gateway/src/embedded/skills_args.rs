//! Skills' ordered anonymous Go argument structs, before any skill I/O.
use super::Error;
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::{Value, json, value::RawValue};
use std::fmt;

struct Fields(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = Fields;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Fields, E> {
                Ok(Fields(Vec::new()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    fields.push(pair);
                }
                Ok(Fields(fields))
            }
        }
        decoder.deserialize_any(Ordered)
    }
}

fn kind(raw: &str) -> &'static str {
    match raw.as_bytes().first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        _ => "number",
    }
}
fn specs(name: &str) -> Vec<(&'static str, &'static str)> {
    match name {
        "skills_profile_resolve" => vec![("name", "string")],
        "skills_targets_status" => vec![("scope", "string")],
        "skills_discover_sources" => vec![("paths", "[]string"), ("scope", "string")],
        "skills_history" => vec![("name", "string"), ("limit", "int")],
        "skills_restore" => vec![
            ("name", "string"),
            ("rev", "string"),
            ("dry_run", "*bool"),
            ("allow_dirty", "bool"),
            ("sync", "bool"),
        ],
        "skills_render_plan" => vec![
            ("target", "string"),
            ("profile", "string"),
            ("dry_run", "*bool"),
        ],
        "skills_install" => vec![
            ("target", "string"),
            ("scope", "string"),
            ("dry_run", "*bool"),
            ("profile", "string"),
        ],
        _ => vec![("path", "string"), ("name", "string")],
    }
}
fn descriptor(fields: &[(&str, &str)]) -> String {
    let members = fields
        .iter()
        .map(|(name, ty)| {
            let native = name
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    })
                })
                .collect::<String>();
            format!("{native} {ty} \"json:\\\"{name}\\\"\"")
        })
        .collect::<Vec<_>>();
    format!("struct {{ {} }}", members.join("; "))
}
fn parse(raw: &str, specs: &[(&str, &str)]) -> Result<Value, Error> {
    let repaired = symbrain_skills::wire::repair_argument_strings(raw.as_bytes());
    let fields = serde_json::from_str::<Fields>(&repaired).map_err(|_| {
        Error::validation(
            "parse arguments",
            format!(
                "json: cannot unmarshal {} into Go value of type {}",
                kind(raw.trim_start()),
                descriptor(specs)
            ),
        )
    })?;
    let mut output = serde_json::Map::new();
    let mut first = None;
    let mut slots = Vec::<String>::new();
    for (key, raw) in fields.0 {
        let key: String = key
            .chars()
            .map(|ch| match ch {
                '\u{212a}' => 'k',
                '\u{017f}' => 's',
                _ => ch.to_ascii_lowercase(),
            })
            .collect();
        let Some((key, ty)) = specs.iter().find(|(name, _)| *name == key) else {
            continue;
        };
        let raw = raw.get();
        if raw == "null" {
            if *ty == "*bool" {
                output.remove(*key);
            }
            if *ty == "[]string" {
                slots.clear();
                output.insert((*key).into(), Value::Null);
            }
            continue;
        }
        let value = match *ty {
            "string" => symbrain_skills::wire::decode_string(raw.as_bytes()).ok().map(Value::String),
            "bool" | "*bool" => serde_json::from_str::<bool>(raw).ok().map(Value::Bool),
            "int" => serde_json::from_str::<i64>(raw).ok().map(|number| json!(number)),
            "[]string" => serde_json::from_str::<Vec<Box<RawValue>>>(raw).ok().map(|values| {
                if values.is_empty() { slots.clear(); }
                for (index, raw) in values.iter().enumerate() {
                    if index == slots.len() { slots.push(String::new()); }
                    if raw.get() == "null" { continue; }
                    match symbrain_skills::wire::decode_string(raw.get().as_bytes()) {
                        Ok(value) => slots[index] = value,
                        Err(_) if first.is_none() => first = Some(format!("json: cannot unmarshal {} into Go struct field .{key} of type string", kind(raw.get()))),
                        Err(_) => {}
                    }
                }
                json!(slots[..values.len()])
            }),
            _ => None,
        };
        if let Some(value) = value {
            output.insert((*key).into(), value);
        } else if first.is_none() {
            let from = if *ty == "int" && kind(raw) == "number" {
                format!("number {raw}")
            } else {
                kind(raw).into()
            };
            first = Some(format!(
                "json: cannot unmarshal {from} into Go struct field .{key} of type {}",
                ty.trim_start_matches('*')
            ));
        }
    }
    if let Some(error) = first {
        return Err(Error::validation("parse arguments", error));
    }
    Ok(Value::Object(output))
}

pub(super) fn decode(name: &str, raw: Option<&RawValue>) -> Result<Value, Error> {
    if matches!(name, "skills_list" | "skills_profile_list") {
        return Ok(json!({}));
    }
    let raw = raw
        .ok_or_else(|| Error::validation("parse arguments", "unexpected end of JSON input"))?
        .get();
    let mut value = parse(raw, &specs(name))?;
    if matches!(name, "skills_render_plan" | "skills_install")
        && value
            .get("profile")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        let bundle = parse(raw, &[("path", "string"), ("name", "string")])?;
        value
            .as_object_mut()
            .expect("arguments")
            .extend(bundle.as_object().expect("bundle").clone());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_error_survives_later_correct_value_and_null_strings_retain() {
        let raw = RawValue::from_string(r#"{"name":4,"name":"demo"}"#.into()).unwrap();
        assert!(decode("skills_history", Some(&raw)).is_err());
        let raw = RawValue::from_string(r#"{"Name":"demo","name":null,"limit":2}"#.into()).unwrap();
        assert_eq!(
            decode("skills_history", Some(&raw)).unwrap()["name"],
            "demo"
        );
    }
    #[test]
    fn profile_branch_ignores_bundle_fields_but_default_branch_checks_them() {
        let raw = RawValue::from_string(r#"{"profile":"work","path":4}"#.into()).unwrap();
        assert!(decode("skills_render_plan", Some(&raw)).is_ok());
        let raw = RawValue::from_string(r#"{"profile":"","path":4}"#.into()).unwrap();
        assert!(decode("skills_render_plan", Some(&raw)).is_err());
    }
}

#[cfg(test)]
mod raw_key_tests {
    use super::*;
    #[test]
    fn ignored_lone_surrogate_key_and_huge_number_reach_original_struct_decoder() {
        let raw = RawValue::from_string(r#"{"\ud800":1e9999,"name":"demo"}"#.into()).unwrap();
        assert_eq!(
            decode("skills_history", Some(&raw)).unwrap()["name"],
            "demo"
        );
        let raw = RawValue::from_string(r#"{"name":"\ud800"}"#.into()).unwrap();
        assert_eq!(
            decode("skills_history", Some(&raw)).unwrap()["name"],
            "\u{fffd}"
        );
        let raw = RawValue::from_string(r#"{"limit":1e9999}"#.into()).unwrap();
        assert!(decode("skills_history", Some(&raw)).is_err());
    }
    #[test]
    fn newly_admitted_strings_keep_shared_audit_redaction() {
        let raw = symbrain_skills::wire::repair_argument_strings(
            br#"{"name":"\ud800","password":"owned-placeholder"}"#,
        );
        let (keys, values) =
            symbrain_audit::redact_args("skills", "skills_history", raw.as_bytes(), true);
        assert_eq!(keys, "name,password");
        assert_eq!(values, "name=\u{fffd},password=[redacted]");
        let raw =
            symbrain_skills::wire::repair_argument_strings(br#"{"ignored":1e9999,"name":"demo"}"#);
        assert_eq!(
            symbrain_audit::redact_args("skills", "skills_history", raw.as_bytes(), true),
            (String::new(), String::new())
        );
    }
}
