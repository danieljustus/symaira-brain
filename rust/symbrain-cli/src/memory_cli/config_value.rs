//! Go configkit conversions: validation is observable even for unused fields.

use super::config_schema::Kind;

pub(super) fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

pub(super) fn validate_env(kind: Kind, value: &str) -> Option<()> {
    match kind {
        Kind::String | Kind::Strings | Kind::Map => Some(()),
        Kind::Integer => parse_integer(value),
        Kind::Float => super::config_float::validate(value),
        Kind::Bool | Kind::PointerBool => parse_bool(value).map(|_| ()),
    }
}

pub(super) fn validate_item(kind: Kind, item: &toml_edit::Item) -> Option<()> {
    if let Some(text) = item.as_str() {
        return if matches!(kind, Kind::Strings | Kind::Map) {
            None
        } else {
            validate_env(kind, text)
        };
    }
    match kind {
        Kind::Integer | Kind::Float if item.is_integer() || item.is_float() => Some(()),
        Kind::Bool | Kind::PointerBool if item.is_bool() => Some(()),
        Kind::Strings => item
            .as_array()?
            .iter()
            .all(toml_edit::Value::is_str)
            .then_some(()),
        _ => None,
    }
}

fn parse_integer(text: &str) -> Option<()> {
    let digits = text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('+'))
        .unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|value| value.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok().map(|_| ())
}
