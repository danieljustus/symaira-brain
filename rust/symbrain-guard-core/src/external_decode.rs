//! Ordered external-request field decoding with Go null/type semantics.
use super::{ExternalDecisionRequest, empty_request};
use serde::de::{Deserializer, MapAccess, Visitor};
use std::fmt;

#[path = "external_deadline.rs"]
mod deadline_parser;

pub(super) struct DecodedRequest {
    pub(super) request: ExternalDecisionRequest,
    pub(super) type_error: Option<String>,
}

struct RequestVisitor<'a> {
    deadlines: std::collections::VecDeque<&'a [u8]>,
}
impl<'de> Visitor<'de> for RequestVisitor<'_> {
    type Value = DecodedRequest;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut request = empty_request();
        let mut type_error = None;
        // Go reuses the backing slice across duplicate array fields.
        // Null elements retain existing slots, even beyond a shorter
        // intervening array's length; null/empty arrays reset the slice.
        let mut warning_slots = Vec::<String>::new();
        while let Some(key) = map.next_key::<String>()? {
            let key = fold_json_key(&key);
            let raw = map.next_value::<Box<serde_json::value::RawValue>>()?;
            if !matches!(
                key.as_str(),
                "command" | "risk_class" | "domain" | "warnings" | "deadline"
            ) {
                continue;
            }
            // RawValue validates syntax without rounding or overflowing ignored
            // numbers. Known fields need only their type, strings, or array slots.
            let value =
                field_value(raw.get(), key == "warnings").map_err(serde::de::Error::custom)?;
            let error = |field: &str, ty: &str| {
                format!(
                    "json: cannot unmarshal {} into Go struct field request.{} of type {}",
                    value_type(&value),
                    field,
                    ty
                )
            };
            match key.as_str() {
                "command" | "risk_class" | "domain" => {
                    if value.is_null() {
                        continue;
                    }
                    if let Some(value) = value.as_str() {
                        match key.as_str() {
                            "command" => value.clone_into(&mut request.command),
                            "risk_class" => value.clone_into(&mut request.risk_class),
                            _ => value.clone_into(&mut request.domain),
                        }
                    } else if type_error.is_none() {
                        type_error = Some(error(key.as_str(), "string"));
                    }
                }
                "warnings" => {
                    if value.is_null() {
                        request.warnings = None;
                        warning_slots.clear();
                    } else if let Some(values) = value.as_array() {
                        if values.is_empty() {
                            warning_slots.clear();
                        }
                        for (index, element) in values.iter().enumerate() {
                            if index == warning_slots.len() {
                                warning_slots.push(String::new());
                            }
                            if let Some(text) = element.as_str() {
                                text.clone_into(&mut warning_slots[index]);
                            } else if !element.is_null() && type_error.is_none() {
                                type_error = Some(format!(
                                    "json: cannot unmarshal {} into Go struct field request.warnings of type string",
                                    value_type(element)
                                ));
                            }
                        }
                        request.warnings = Some(warning_slots[..values.len()].to_vec());
                    } else if type_error.is_none() {
                        type_error = Some(error("warnings", "[]string"));
                    }
                }
                "deadline" => {
                    // Go time.Time.UnmarshalJSON parses the quoted bytes, not
                    // a JSON-unescaped string, and its error stops field decoding.
                    let raw = self
                        .deadlines
                        .pop_front()
                        .expect("validated top-level deadline");
                    if let Err(error) = decode_deadline(raw, &mut request) {
                        type_error = Some(error);
                        // Consume remaining object members without changing the
                        // first time error; syntax was checked before this visitor.
                        while map
                            .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                            .is_some()
                        {}
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(DecodedRequest {
            request,
            type_error,
        })
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(DecodedRequest {
            request: empty_request(),
            type_error: None,
        })
    }
}

pub(super) fn decode_request(input: &[u8]) -> Result<DecodedRequest, serde_json::Error> {
    let text = crate::capability_wire::repair_json_strings(input);
    let raw: Box<serde_json::value::RawValue> = serde_json::from_str(&text)?;
    if !raw.get().starts_with('{') && raw.get() != "null" {
        let value = field_value(raw.get(), false)?;
        return Ok(DecodedRequest {
            request: empty_request(),
            type_error: Some(format!(
                "json: cannot unmarshal {} into Go value of type decide.request",
                value_type(&value)
            )),
        });
    }
    let mut deserializer = serde_json::Deserializer::from_str(&text);
    // Retain original time bytes before Unicode replacement: Go's time parser
    // sees literal escapes and invalid UTF-8 and quotes those bytes in errors.
    let mut deadlines = std::collections::VecDeque::new();
    for (key, value) in super::json_syntax::fields(input).map_err(serde::de::Error::custom)? {
        let key: String = serde_json::from_str(&crate::capability_wire::repair_json_strings(key))?;
        if fold_json_key(&key) == "deadline" {
            deadlines.push_back(value);
        }
    }
    let result = deserializer.deserialize_any(RequestVisitor { deadlines })?;
    deserializer.end()?;
    Ok(result)
}

fn field_value(raw: &str, array: bool) -> Result<serde_json::Value, serde_json::Error> {
    use serde_json::Value;
    Ok(match raw.as_bytes()[0] {
        b'"' | b'n' | b't' | b'f' => serde_json::from_str(raw)?,
        b'{' => Value::Object(serde_json::Map::new()),
        b'[' if array => {
            let slots: Vec<Box<serde_json::value::RawValue>> = serde_json::from_str(raw)?;
            Value::Array(
                slots
                    .iter()
                    .map(|s| field_value(s.get(), false))
                    .collect::<Result<_, _>>()?,
            )
        }
        b'[' => Value::Array(Vec::new()),
        _ => Value::Number(0.into()),
    })
}

#[allow(
    clippy::ref_option,
    reason = "Serde skip_serializing_if receives a field reference"
)]
pub(super) fn warnings_empty(value: &Option<Vec<String>>) -> bool {
    value.as_ref().is_none_or(Vec::is_empty)
}

fn decode_deadline(value: &[u8], request: &mut ExternalDecisionRequest) -> Result<(), String> {
    let value = value.trim_ascii();
    match value {
        b"null" => {}
        [b'"', value @ .., b'"'] => request.deadline = Some(deadline_parser::parse(value)?),
        _ => {
            return Err("Time.UnmarshalJSON: input is not a JSON string".to_owned());
        }
    }
    Ok(())
}

fn value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn fold_json_key(key: &str) -> String {
    key.chars()
        .map(|ch| match ch {
            '\u{212a}' => 'k',
            '\u{017f}' => 's',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}
