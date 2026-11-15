//! Narrow scrubbing of credential-shaped text that is echoed back to a user,
//! a log or a journal (SEC-007).
//!
//! Unlike the daemon's error redactor, which also treats `key value` and
//! `key: value` as assignments, this scrubber only touches text that is
//! unambiguously a credential: the password of URL userinfo and the value of
//! `name=value` pairs whose name is a secret name. That keeps diagnostics such
//! as `auth login: no form` or `unknown flag: --token` intact, so it is safe to
//! run over whole error messages, help-adjacent output and journal fields.

/// Replacement for a scrubbed value.
pub const MASK: &str = "[REDACTED]";

/// Names that mark the value of a `name=value` pair as a credential.
const SECRET_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "access_key",
    "private_key",
    "credential",
    "authorization",
    "cookie",
    "encryption_key",
];
const SECRET_EXACT: &[&str] = &["pass", "pwd", "auth", "key", "sig", "signature", "sid"];

/// True when `name` (a flag, query parameter or setting name) carries a secret.
#[must_use]
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let lower = lower.trim_start_matches('-');
    SECRET_EXACT.contains(&lower) || SECRET_FRAGMENTS.iter().any(|part| lower.contains(part))
}

/// Scrubs URL userinfo passwords and the values of secret-named `name=value`
/// pairs (including URL query parameters). Other text is returned unchanged.
#[must_use]
pub fn scrub(input: &str) -> String {
    if !input.contains(['=', '@']) {
        return input.to_owned();
    }
    scrub_assignments(&scrub_userinfo(input))
}

fn scrub_userinfo(input: &str) -> String {
    let mut output = input.to_owned();
    let mut cursor = 0;
    while let Some(relative) = output[cursor..].find("://") {
        let authority_start = cursor + relative + 3;
        let authority_end = output[authority_start..]
            .find(['/', '?', '#', ' ', '\t', '\n', '\r', '"', '\''])
            .map_or(output.len(), |offset| authority_start + offset);
        let authority = &output[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            let userinfo_end = authority_start + at;
            if let Some(colon) = output[authority_start..userinfo_end].find(':') {
                let secret_start = authority_start + colon + 1;
                output.replace_range(secret_start..userinfo_end, MASK);
                cursor = secret_start + MASK.len();
                continue;
            }
        }
        cursor = authority_end;
    }
    output
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn scrub_assignments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut copied = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'=' {
            index += 1;
            continue;
        }
        let mut name_start = index;
        while name_start > copied && is_name_byte(bytes[name_start - 1]) {
            name_start -= 1;
        }
        let value_start = index + 1;
        if name_start < index && is_secret_name(&input[name_start..index]) {
            let value_end = value_end(input, value_start);
            if value_end > value_start && !input[value_start..].starts_with(MASK) {
                output.push_str(&input[copied..value_start]);
                output.push_str(MASK);
                copied = value_end;
                index = value_end;
                continue;
            }
        }
        index += 1;
    }
    output.push_str(&input[copied..]);
    output
}

fn value_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(byte, b'"' | b'\''));
    if let Some(quote) = quote {
        return bytes[start + 1..]
            .iter()
            .position(|byte| *byte == quote)
            .map_or(bytes.len(), |offset| start + 1 + offset + 1);
    }
    bytes[start..]
        .iter()
        .position(|byte| {
            matches!(
                byte,
                b' ' | b'\t'
                    | b'\r'
                    | b'\n'
                    | b'&'
                    | b','
                    | b';'
                    | b'"'
                    | b'\''
                    | b')'
                    | b']'
                    | b'}'
                    | b'#'
            )
        })
        .map_or(bytes.len(), |offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_userinfo_query_and_assignments_but_not_prose() {
        let cases = [
            (
                "https://user:hunter2@example.com/p?token=abc&x=1&api_key=def#f",
                "https://user:[REDACTED]@example.com/p?token=[REDACTED]&x=1&api_key=[REDACTED]#f",
            ),
            (
                "unknown command \"password=s3\"",
                "unknown command \"password=[REDACTED]\"",
            ),
            ("Cookie=\"a b\" next", "Cookie=[REDACTED] next"),
            ("auth login: no form", "auth login: no form"),
            ("unknown flag: --token", "unknown flag: --token"),
            ("--token=abc rest", "--token=[REDACTED] rest"),
            ("user@example.com", "user@example.com"),
            ("http://host:8080/x", "http://host:8080/x"),
            ("name=value other=thing", "name=value other=thing"),
            ("token=[REDACTED]", "token=[REDACTED]"),
        ];
        for (input, expected) in cases {
            assert_eq!(scrub(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn secret_names_match_by_fragment_or_exact_name() {
        for name in [
            "password",
            "X-Api-Key",
            "--token",
            "access_token",
            "sig",
            "KEY",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["name", "url", "keyboard", "session", "engine"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }
}
