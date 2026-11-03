//! Port of `internal/memory/security/pii.go`: the response-boundary and
//! write-path PII guard of the shipped memory server.
//!
//! Go's `regexp` is ASCII for `\s`, `\d` and `\b`; the Rust `regex` crate is
//! Unicode by default, so every pattern below spells those classes out. Match
//! order, replacement text and the quirks of the shipped dispatcher are kept:
//! a phone number is not numeric (it contains `+`) and therefore ends up as
//! `[REDACTED_API_KEY]`, exactly like the shipped output.

use regex::{Captures, Regex};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Go's `\s` without Unicode: `[\t\n\f\r ]`.
macro_rules! ws {
    () => {
        r"\t\n\x0C\r "
    };
}

struct Pattern {
    label: &'static str,
    re: Regex,
}

#[allow(clippy::too_many_lines)] // The shipped pattern table, in order.
fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let sources: [(&str, String); 22] = [
            (
                "url_credential",
                concat!(
                    r"(?i)(?:https?|ftp|sftp|amqps?|mongodb(?:\\+srv)?|postgres(?:ql)?|mysql|redis)://[^",
                    ws!(),
                    r":@]+:[^",
                    ws!(),
                    r"@]+@[^",
                    ws!(),
                    r"]+"
                )
                .to_owned(),
            ),
            (
                "openai_project_key",
                r"(?i)(?:sk-proj-[a-zA-Z0-9]{32,})".to_owned(),
            ),
            (
                "github_token",
                r"(?i)(?:ghp_[a-zA-Z0-9]{36}|gho_[a-zA-Z0-9]{36}|ghs_[a-zA-Z0-9]{36}|ghr_[a-zA-Z0-9]{36})"
                    .to_owned(),
            ),
            (
                "google_api_key",
                r"(?i)(?:AIzaSy[a-zA-Z0-9-_]{33})".to_owned(),
            ),
            (
                "bearer_token",
                concat!(r"(?i)(?:bearer[", ws!(), r"]+[a-zA-Z0-9-_\.]{20,})").to_owned(),
            ),
            (
                "aws_credential",
                r"(?i)(?:AKIA[A-Z0-9]{16}:[A-Za-z0-9/+=]{40})".to_owned(),
            ),
            ("aws_access_key_id", r"(?i)(?:AKIA[A-Z0-9]{16})".to_owned()),
            (
                "slack_token",
                r"(?i)(?:xox[abposr]-[a-zA-Z0-9-]{10,60})".to_owned(),
            ),
            (
                "stripe_live_key",
                r"(?i)(?:sk_live_[a-zA-Z0-9]{24,})".to_owned(),
            ),
            (
                "gitlab_pat",
                r"(?i)(?:glpat-[A-Za-z0-9_-]{20,})".to_owned(),
            ),
            ("npm_token", r"(?i)(?:npm_[A-Za-z0-9]{36})".to_owned()),
            (
                "fcm_server_key",
                r"(?i)(?:AAAA[A-Za-z0-9_-]{120,})".to_owned(),
            ),
            (
                "http_basic_auth",
                concat!(r"(?i)(?:basic[", ws!(), r"]+[A-Za-z0-9+/=]{20,})").to_owned(),
            ),
            (
                "docker_config_auth",
                concat!(
                    r#"(?i)(?:"auth"["#,
                    ws!(),
                    r#"]*:["#,
                    ws!(),
                    r#"]*"[A-Za-z0-9+/=]{20,}")"#
                )
                .to_owned(),
            ),
            (
                "private_key",
                concat!(
                    r"(?i)(?:-----BEGIN[",
                    ws!(),
                    r"](?:RSA[",
                    ws!(),
                    r"]|EC[",
                    ws!(),
                    r"]|DSA[",
                    ws!(),
                    r"]|OPENSSH[",
                    ws!(),
                    r"])?PRIVATE[",
                    ws!(),
                    r"]KEY-----[A-Za-z0-9+/=\n",
                    ws!(),
                    r"]+-----END[",
                    ws!(),
                    r"](?:RSA[",
                    ws!(),
                    r"]|EC[",
                    ws!(),
                    r"]|DSA[",
                    ws!(),
                    r"]|OPENSSH[",
                    ws!(),
                    r"])?PRIVATE[",
                    ws!(),
                    r"]KEY-----)"
                )
                .to_owned(),
            ),
            (
                "api_key",
                r"(?i)(?:sk-[a-zA-Z0-9]{20,})".to_owned(),
            ),
            (
                "jwt_token",
                r"(?i)(?:eyJ[a-zA-Z0-9_-]{10,}\.eyJ[a-zA-Z0-9_-]{10,})".to_owned(),
            ),
            (
                "ssh_public_key",
                concat!(
                    r"(?i)(?:ssh-(?:rsa|ed25519|dss)[",
                    ws!(),
                    r"]+[A-Za-z0-9+/=]{40,})"
                )
                .to_owned(),
            ),
            (
                "azure_account_key",
                r"(?i)(?:AccountKey=[A-Za-z0-9+/]{86}[AEIMQUYcgkosw048]=)".to_owned(),
            ),
            (
                "gcp_private_key",
                concat!(
                    r#"(?i)"private_key"["#,
                    ws!(),
                    r#"]*:["#,
                    ws!(),
                    r#"]*"-----BEGIN["#,
                    ws!(),
                    r"](?:RSA[",
                    ws!(),
                    r"]|EC[",
                    ws!(),
                    r"]|DSA[",
                    ws!(),
                    r"]|OPENSSH[",
                    ws!(),
                    r#"])?PRIVATE["#,
                    ws!(),
                    r#"]KEY-----[^"]*""#
                )
                .to_owned(),
            ),
            (
                "email",
                r"(?i)[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}".to_owned(),
            ),
            (
                "phone_number",
                concat!(
                    r"\+[0-9]{1,3}[",
                    ws!(),
                    r".-]?\(?[0-9]{2,4}\)?[",
                    ws!(),
                    r".-]?[0-9]{3,4}[",
                    ws!(),
                    r".-]?[0-9]{3,4}"
                )
                .to_owned(),
            ),
        ];
        let mut compiled = sources
            .into_iter()
            .map(|(label, source)| Pattern {
                label,
                re: Regex::new(&source).expect("static PII pattern"),
            })
            .collect::<Vec<_>>();
        compiled.push(Pattern {
            label: "credit_card",
            re: Regex::new(
                r"(?-u:\b)(?:4[0-9]{3}|5[1-5][0-9]{2}|2[2-7][0-9]{2}|3[47][0-9]{2}|62[0-9]{2}|65[0-9]{2}|6011)(?:[ -]?[0-9]){9,12}(?-u:\b)",
            )
            .expect("static PII pattern"),
        });
        compiled
    })
}

fn assignment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)(?:(?-u:\b)(?:api[_-]?key|auth[_-]?token|access[_-]?token|client[_-]?secret|private[_-]?key|secret|token|password|passwd)(?-u:\b)[",
            ws!(),
            r"]*[:=][",
            ws!(),
            r"]*)([A-Za-z0-9_\-\.]{20,})"
        ))
        .expect("static assignment pattern")
    })
}

/// Redacts PII from `text`; returns the cleaned text and the pattern labels
/// of every redaction (for audit events, never the matched values).
#[must_use]
pub fn redact_with_labels(text: &str) -> (String, Vec<&'static str>) {
    let mut cleaned = text.to_owned();
    let mut labels = Vec::new();
    for pattern in patterns() {
        cleaned = pattern
            .re
            .replace_all(&cleaned, |captures: &Captures<'_>| {
                let matched = &captures[0];
                if matched.contains("://") && matched.contains('@') {
                    labels.push(pattern.label);
                    return "[REDACTED_URL_CREDENTIALS]".to_owned();
                }
                if is_numeric(matched) {
                    if luhn(matched) {
                        labels.push(pattern.label);
                        return "[REDACTED_CARD_NUMBER]".to_owned();
                    }
                    return matched.to_owned();
                }
                labels.push(pattern.label);
                if matched.contains('@') {
                    "[REDACTED_EMAIL]".to_owned()
                } else {
                    "[REDACTED_API_KEY]".to_owned()
                }
            })
            .into_owned();
    }
    let cleaned = assignment()
        .replace_all(&cleaned, |captures: &Captures<'_>| {
            let matched = &captures[0];
            let mut start = 0;
            for (index, character) in matched.char_indices() {
                if character == ':' || character == '=' {
                    start = index + 1;
                    break;
                }
            }
            while matched
                .as_bytes()
                .get(start)
                .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
            {
                start += 1;
            }
            let value = &matched[start..];
            if likely_secret(value) {
                labels.push("high_entropy_secret");
                format!("{}[REDACTED_API_KEY]", &matched[..start])
            } else {
                matched.to_owned()
            }
        })
        .into_owned();
    (cleaned, labels)
}

/// `security.Redact`.
#[must_use]
pub fn redact(text: &str) -> String {
    redact_with_labels(text).0
}

/// `security.RedactMap`: values are redacted, keys are preserved.
#[must_use]
pub fn redact_map(map: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    map.iter()
        .map(|(key, value)| (key.clone(), redact(value)))
        .collect()
}

/// `isNumeric`: only digits, spaces and hyphens (an empty string counts).
fn is_numeric(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_digit() || c == ' ' || c == '-')
}

/// Luhn checksum over the digit characters, 13 to 16 digits.
fn luhn(text: &str) -> bool {
    let digits = text
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect::<Vec<_>>();
    if digits.len() < 13 || digits.len() > 16 {
        return false;
    }
    let mut sum = 0;
    let mut double = false;
    for digit in digits.iter().rev() {
        let mut d = *digit;
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

fn all_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// `isLikelySecret`: the conservative entropy test of the shipped guard.
fn likely_secret(value: &str) -> bool {
    if value.len() < 20 || all_hex(value, 40) || all_hex(value, 64) || is_uuid(value) {
        return false;
    }
    let classes = [
        value.bytes().any(|b| b.is_ascii_lowercase()),
        value.bytes().any(|b| b.is_ascii_uppercase()),
        value.bytes().any(|b| b.is_ascii_digit()),
        value.bytes().any(|b| matches!(b, b'_' | b'-' | b'.')),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if classes < 2 {
        return false;
    }
    let mut frequencies = BTreeMap::new();
    for character in value.chars() {
        *frequencies.entry(character).or_insert(0_u32) += 1;
    }
    if frequencies.len() < 10 {
        return false;
    }
    // The shipped code divides by the byte length, not the rune count.
    #[allow(clippy::cast_precision_loss)]
    let length = value.len() as f64;
    let mut entropy = 0.0_f64;
    for count in frequencies.values() {
        let p = f64::from(*count) / length;
        entropy -= p * p.log2();
    }
    entropy >= 3.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_each_class_like_the_shipped_guard() {
        assert_eq!(
            redact("mail me at a.b@example.com now"),
            "mail me at [REDACTED_EMAIL] now"
        );
        assert_eq!(
            redact("db postgres://user:pw@host:5432/x end"),
            "db [REDACTED_URL_CREDENTIALS] end"
        );
        assert_eq!(
            redact("key sk-abcdefghijklmnopqrstuv"),
            "key [REDACTED_API_KEY]"
        );
        // Phone numbers contain '+', are not numeric and end up as API keys.
        assert_eq!(redact("call +49 170 1234567"), "call [REDACTED_API_KEY]");
        assert_eq!(
            redact("card 4111 1111 1111 1111 ok"),
            "card [REDACTED_CARD_NUMBER] ok"
        );
        assert_eq!(
            redact("not 4111 1111 1111 1112 ok"),
            "not 4111 1111 1111 1112 ok"
        );
    }

    #[test]
    fn mongodb_srv_quantifier_applies_to_the_backslash() {
        // Go's raw string `\\+` is "one or more backslashes".
        assert_eq!(
            redact("mongodb\\srv://u:p@h/x"),
            "[REDACTED_URL_CREDENTIALS]"
        );
    }

    #[test]
    fn assignment_entropy_gate() {
        assert_eq!(
            redact("token: Abc123_def-456.GhIjKlMnOp"),
            "token: [REDACTED_API_KEY]"
        );
        let uuid = "password = 123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(redact(uuid), uuid);
        let hex = "secret=0123456789abcdef0123456789abcdef01234567";
        assert_eq!(redact(hex), hex);
    }

    #[test]
    fn ascii_classes_do_not_match_unicode_whitespace() {
        // U+00A0 is not Go `\s`, so the bearer pattern must not match it.
        let text = "bearer\u{a0}abcdefghijklmnopqrstuvwxyz";
        assert_eq!(redact(text), text);
    }
}
