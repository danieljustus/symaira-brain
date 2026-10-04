//! Ordered outer tools/call admission, scoped to the eleven Skills owners.
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::value::RawValue;
use std::fmt;

#[path = "raw_skills_strings.rs"]
mod strings;
#[path = "raw_skills_syntax.rs"]
mod syntax;

/// Original CoreKit registration order; also scopes raw transport admission.
pub const SKILLS_TOOL_NAMES: &[&str] = &[
    "skills_list",
    "skills_inspect",
    "skills_validate",
    "skills_profile_list",
    "skills_profile_resolve",
    "skills_render_plan",
    "skills_install",
    "skills_discover_sources",
    "skills_history",
    "skills_restore",
    "skills_targets_status",
];

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

fn fold_key(key: &str) -> String {
    key.chars()
        .map(|ch| match ch {
            '\u{212a}' => 'k',
            '\u{017f}' => 's',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}

fn kind(raw: &str) -> &'static str {
    match raw.trim_start().as_bytes().first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        _ => "number",
    }
}
fn field_error(raw: &str, field: &str, ty: &str) -> String {
    format!(
        "Invalid params: json: cannot unmarshal {} into Go struct field .{field} of type {ty}",
        kind(raw)
    )
}

// Map[string]any numbers use float64, even in otherwise ignored metadata.
// Preserve encounter order rather than validating a duplicate-free Value.
fn validate_any(raw: &str) -> Option<String> {
    let mut first = None;
    let _ = syntax::scan(raw.as_bytes(), |number| {
        if first.is_some() {
            return;
        }
        let number = std::str::from_utf8(number).expect("syntax-validated ASCII number");
        if symbrain_core::go_json_float::parse_finite(number).is_none() {
            first = Some(format!(
                "Invalid params: json: cannot unmarshal number {number} into Go struct field ._meta of type float64"
            ));
        }
    });
    first
}

/// Known Skills name and its separately decoded raw handler arguments.
pub type RawSkillsCall = (String, Option<Box<RawValue>>);

/// Validates Go's Name string / Arguments RawMessage / Meta map[string]any.
/// A recognized call retains its first outer error and must never execute.
/// None leaves syntactically valid other RPC owners on their original path.
/// Complete syntax errors precede owner matching and typed field decoding.
#[must_use]
pub fn raw_skills_params(params: Option<&RawValue>) -> Option<Result<RawSkillsCall, String>> {
    let bytes = params?.get().as_bytes();
    if let Err(error) = syntax::validate(bytes) {
        return Some(Err(format!("Invalid params: {error}")));
    }
    let repaired = strings::transport(bytes, 1)?;
    let fields = serde_json::from_str::<Fields>(&repaired).ok()?;
    let (mut name, mut arguments, mut first) = (String::new(), None, None);
    for (key, value) in fields.0 {
        let raw = value.get();
        let key = fold_key(&key);
        if key == "name" {
            if raw == "null" {
                continue;
            }
            match serde_json::from_str::<String>(&strings::repair(raw.as_bytes(), true)) {
                Ok(value) => name = value,
                Err(_) => {
                    first.get_or_insert_with(|| field_error(raw, "name", "string"));
                }
            }
        } else if key == "arguments" {
            arguments = Some(value);
        } else if key == "_meta" && raw != "null" {
            let failure = if kind(raw) == "object" {
                validate_any(raw)
            } else {
                Some(field_error(raw, "_meta", "map[string]interface {}"))
            };
            if let Some(failure) = failure {
                first.get_or_insert(failure);
            }
        }
    }
    SKILLS_TOOL_NAMES
        .contains(&name.as_str())
        .then(|| first.map_or_else(|| Ok((name, arguments)), Err))
}

pub(crate) fn transport_request(bytes: &[u8]) -> Option<Result<crate::Request, String>> {
    if let Err(error) = syntax::validate(bytes) {
        return Some(Err(error));
    }
    let repaired = strings::transport(bytes, 2)?;
    let fields = serde_json::from_str::<Fields>(&repaired).ok()?;
    let (mut jsonrpc, mut method, mut params, mut id, mut has_id, mut first) =
        (String::new(), String::new(), None, None, false, None);
    for (key, raw) in fields.0 {
        let key_folded = fold_key(&key);
        if key_folded == "jsonrpc" || key_folded == "method" {
            if raw.get() == "null" {
                continue;
            }
            match serde_json::from_str::<String>(&strings::repair(raw.get().as_bytes(), true)) {
                Ok(value) => {
                    if key_folded == "jsonrpc" {
                        jsonrpc = value;
                    } else {
                        method = value;
                    }
                }
                Err(_) => {
                    first.get_or_insert_with(|| format!("json: cannot unmarshal {} into Go struct field requestAlias.{key_folded} of type string", kind(raw.get())));
                }
            }
        } else if key_folded == "params" {
            params = Some(raw);
        } else if key_folded == "id" {
            has_id |= key == "id";
            let fixed = strings::repair(raw.get().as_bytes(), true);
            match serde_json::from_str::<serde_json::Value>(&fixed) {
                Ok(value) => id = (!value.is_null()).then_some(value),
                Err(error) => {
                    first.get_or_insert_with(|| error.to_string());
                }
            }
        }
    }
    if jsonrpc != crate::JSONRPC_VERSION
        || method != "tools/call"
        || raw_skills_params(params.as_deref()).is_none()
    {
        return None;
    }
    Some(first.map_or_else(
        || {
            Ok(crate::Request {
                jsonrpc,
                method,
                id,
                params,
                has_id,
            })
        },
        Err,
    ))
}

pub(crate) fn trim_go_space(raw: &[u8]) -> &[u8] {
    strings::trim_go_space(raw)
}

#[cfg(test)]
#[path = "raw_skills_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "raw_skills_boundary_tests.rs"]
mod boundary_tests;
