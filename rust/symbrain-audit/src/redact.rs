use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use serde_json::{Map, Number, Value};

const MAX_ARG_VALUE_LEN: usize = 256;
const MAX_JSON_DEPTH: usize = 128;
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pass",
    "pwd",
    "token",
    "access_token",
    "access-token",
    "accesstoken",
    "refresh_token",
    "refresh-token",
    "refreshtoken",
    "secret",
    "client_secret",
    "client-secret",
    "clientsecret",
    "api_key",
    "api-key",
    "apikey",
    "authorization",
    "auth",
    "bearer",
    "credential",
    "credentials",
    "private_key",
    "private-key",
    "privatekey",
    "passphrase",
    "content",
];

/// Applies the Go audit redaction policy to raw JSON arguments.
#[must_use]
pub fn redact_args(server: &str, tool: &str, args: &[u8], verbose: bool) -> (String, String) {
    if args.is_empty() || server == "vault" || tool.starts_with("vault_") {
        return (String::new(), String::new());
    }
    let Ok(raw) = serde_json::from_slice::<Box<RawValue>>(args) else {
        return (String::new(), String::new());
    };
    let Ok(Value::Object(values)) = parse_go_value(&raw, 0) else {
        return (String::new(), String::new());
    };
    if values.is_empty() {
        return (String::new(), String::new());
    }

    // Go map iteration is deliberately unspecified. Sorting gives Rust a stable
    // representation while preserving the public key/value set contract.
    let mut keys = values.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let key_list = keys.join(",");
    if !verbose {
        return (key_list, String::new());
    }

    let redacted = redact_map(&values);
    let rendered = keys
        .iter()
        .map(|key| {
            let mut value = go_format(&redacted[key]);
            value = truncate_go_bytes(&value, MAX_ARG_VALUE_LEN);
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join(",");
    (key_list, rendered)
}

/// Parses numbers as Go's `encoding/json` does without interpreting real
/// object keys as `serde_json`'s `arbitrary_precision` internal number marker.
fn parse_go_value(raw: &RawValue, depth: usize) -> Result<Value, ()> {
    let json = raw.get();
    let first = json.trim_start().as_bytes().first().copied().ok_or(())?;
    match first {
        b'{' => {
            if depth >= MAX_JSON_DEPTH {
                return Err(());
            }
            let RawObject(entries) = serde_json::from_str(json).map_err(|_| ())?;
            let mut values = Map::new();
            for (key, value) in entries {
                // Parse each value before insertion so an overflowing earlier
                // duplicate key is still rejected, as in Go's JSON decoder.
                values.insert(key, parse_go_value(&value, depth + 1)?);
            }
            Ok(Value::Object(values))
        }
        b'[' => {
            if depth >= MAX_JSON_DEPTH {
                return Err(());
            }
            let values: Vec<Box<RawValue>> = serde_json::from_str(json).map_err(|_| ())?;
            values
                .iter()
                .map(|value| parse_go_value(value, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        b'-' | b'0'..=b'9' => {
            let number = json.parse::<f64>().map_err(|_| ())?;
            if !number.is_finite() {
                return Err(());
            }
            Number::from_f64(number).map(Value::Number).ok_or(())
        }
        _ => serde_json::from_str(json).map_err(|_| ()),
    }
}

struct RawObject(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for RawObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawObjectVisitor;

        impl<'de> Visitor<'de> for RawObjectVisitor {
            type Value = RawObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    entries.push((key, map.next_value::<Box<RawValue>>()?));
                }
                Ok(RawObject(entries))
            }
        }

        deserializer.deserialize_map(RawObjectVisitor)
    }
}

fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS
        .iter()
        .any(|candidate| key.eq_ignore_ascii_case(candidate))
}

fn redact_map(values: &Map<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .map(|(key, value)| {
            let value = if is_sensitive_key(key) {
                Value::String("[redacted]".to_string())
            } else {
                redact_value(value)
            };
            (key.clone(), value)
        })
        .collect()
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(redact_map(values)),
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        other => other.clone(),
    }
}

fn go_format(value: &Value) -> String {
    match value {
        Value::Null => "<nil>".to_string(),
        Value::Bool(value) => value.to_string(),
        // Go's json.Unmarshal decodes every JSON number as float64 before
        // fmt.Sprintf("%v", value), including integer-looking numbers.
        Value::Number(value) => value
            .as_f64()
            .map_or_else(|| value.to_string(), go_format_float),
        Value::String(value) => value.clone(),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(go_format).collect::<Vec<_>>().join(" ")
        ),
        Value::Object(values) => {
            let mut pairs = values.iter().collect::<Vec<_>>();
            pairs.sort_by(|left, right| left.0.cmp(right.0));
            format!(
                "map[{}]",
                pairs
                    .into_iter()
                    .map(|(key, value)| format!("{key}:{}", go_format(value)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
    }
}

fn go_format_float(value: f64) -> String {
    let rendered = value.to_string();
    let (sign, rendered) = rendered
        .strip_prefix('-')
        .map_or(("", rendered.as_str()), |unsigned| ("-", unsigned));
    let (integer, fraction) = rendered.split_once('.').unwrap_or((rendered, ""));
    let decimal_position = integer.len().cast_signed();
    let digits = format!("{integer}{fraction}");
    let first = digits.bytes().position(|digit| digit != b'0');
    let Some(first) = first else {
        return format!("{sign}0");
    };
    let significant = digits[first..].trim_end_matches('0');
    let exponent = decimal_position - first.cast_signed() - 1;

    if !(-4..6).contains(&exponent) {
        let mantissa = if significant.len() == 1 {
            significant.to_string()
        } else {
            format!("{}.{}", &significant[..1], &significant[1..])
        };
        return format!("{sign}{mantissa}e{exponent:+03}");
    }

    let decimal_position = exponent + 1;
    let fixed = if decimal_position <= 0 {
        format!(
            "0.{}{significant}",
            "0".repeat((-decimal_position).cast_unsigned())
        )
    } else if decimal_position.cast_unsigned() >= significant.len() {
        format!(
            "{}{}",
            significant,
            "0".repeat(decimal_position.cast_unsigned() - significant.len())
        )
    } else {
        let position = decimal_position.cast_unsigned();
        format!("{}.{}", &significant[..position], &significant[position..])
    };
    format!("{sign}{fixed}")
}

fn truncate_go_bytes(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut boundary = max;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &value[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_never_logs_arguments() {
        let args = br#"{"password":"hunter2","path":"creds/key"}"#;
        assert_eq!(
            redact_args("vault", "get_entry", args, true),
            (String::new(), String::new())
        );
        assert_eq!(
            redact_args("other", "vault_get_entry", args, true),
            (String::new(), String::new())
        );
    }

    #[test]
    fn non_verbose_logs_sorted_keys_only() {
        assert_eq!(
            redact_args(
                "memory",
                "memory_search",
                br#"{"query":"x","limit":10}"#,
                false
            ),
            ("limit,query".to_string(), String::new())
        );
    }

    #[test]
    fn verbose_recursively_redacts_sensitive_values() {
        let (_, values) = redact_args(
            "foreign",
            "call",
            br#"{"items":[{"TOKEN":"secret"}],"api_key":"key","query":"x"}"#,
            true,
        );
        assert_eq!(
            values,
            "api_key=[redacted],items=[map[TOKEN:[redacted]]],query=x"
        );
        assert!(!values.contains("secret"));
    }

    #[test]
    fn verbose_numbers_match_go_float64_formatting() {
        let (_, values) = redact_args(
            "foreign",
            "call",
            br#"{"fixed_low":1e-4,"fixed_high":1e6,"large":1e20,"lossy":9007199254740993,"negative_zero":-0,"nested":[1.0,2],"one":1.0,"small":1e-5}"#,
            true,
        );
        assert_eq!(
            values,
            "fixed_high=1e+06,fixed_low=0.0001,large=1e+20,lossy=9.007199254740992e+15,negative_zero=-0,nested=[1 2],one=1,small=1e-05"
        );
    }

    #[test]
    fn negative_zero_survives_feature_unified_json_parsing_without_touching_strings() {
        let (_, values) = redact_args(
            "foreign",
            "call",
            br#"{"values":[-0,"-0","escaped quote: \" then -0",-0.0,-0e0,0]}"#,
            true,
        );

        assert_eq!(values, "values=[-0 -0 escaped quote: \" then -0 -0 -0 0]");
    }

    #[test]
    fn arbitrary_precision_internal_number_key_remains_a_user_object_key() {
        let (_, values) = redact_args(
            "foreign",
            "call",
            br#"{"$serde_json::private::Number":"1","nested":{"$serde_json::private::Number":"-0"}}"#,
            true,
        );

        assert_eq!(
            values,
            "$serde_json::private::Number=1,nested=map[$serde_json::private::Number:-0]"
        );
    }

    #[test]
    fn raw_value_recursion_keeps_the_json_depth_bound() {
        let nested_json = |levels: usize| {
            format!(
                "{{\"nested\":{}0{}}}",
                "[".repeat(levels),
                "]".repeat(levels)
            )
        };

        let (keys, values) = redact_args("foreign", "call", nested_json(48).as_bytes(), true);
        assert_eq!(keys, "nested");
        assert!(values.starts_with("nested=[[["));
        assert_eq!(
            redact_args("foreign", "call", nested_json(256).as_bytes(), true),
            (String::new(), String::new())
        );
    }

    #[test]
    fn overflowing_nested_numbers_match_go_json_decode_failure() {
        let huge_integer = format!("{{\"nested\":[{{\"number\":{}}}]}}", "9".repeat(400));
        for args in [
            br#"{"number":1e400}"#.as_slice(),
            huge_integer.as_bytes(),
            br#"{"number":1e400,"number":1}"#.as_slice(),
            br#"{"number":[1e400],"number":[]}"#.as_slice(),
        ] {
            assert_eq!(
                redact_args("foreign", "call", args, true),
                (String::new(), String::new())
            );
        }
    }

    #[test]
    fn malformed_and_non_object_arguments_are_ignored() {
        assert_eq!(
            redact_args("memory", "x", b"bad", true),
            (String::new(), String::new())
        );
        assert_eq!(
            redact_args("memory", "x", b"[]", true),
            (String::new(), String::new())
        );
    }
}
