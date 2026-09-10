use serde_json::{Map, Value};

const MAX_ARG_VALUE_LEN: usize = 256;
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
    let Ok(Value::Object(values)) = serde_json::from_slice::<Value>(args) else {
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
        Value::Number(value) => value.to_string(),
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
