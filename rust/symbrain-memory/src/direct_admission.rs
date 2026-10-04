//! Conservative admission for the proven, non-redacting direct-write lane.

use regex::Regex;
use std::sync::OnceLock;

const PII: &[&str] = &[
    r"(?i)(?:https?|ftp|sftp|amqps?|mongodb(?:\\+srv)?|postgres(?:ql)?|mysql|redis)://[^\s:@]+:[^\s@]+@[^\s]+",
    r"(?i)(?:sk-proj-[a-zA-Z0-9]{32,})",
    r"(?i)(?:ghp_[a-zA-Z0-9]{36}|gho_[a-zA-Z0-9]{36}|ghs_[a-zA-Z0-9]{36}|ghr_[a-zA-Z0-9]{36})",
    r"(?i)(?:AIzaSy[a-zA-Z0-9-_]{33})",
    r"(?i)(?:bearer\s+[a-zA-Z0-9-_\.]{20,})",
    r"(?i)(?:AKIA[A-Z0-9]{16}:[A-Za-z0-9/+=]{40})",
    r"(?i)(?:AKIA[A-Z0-9]{16})",
    r"(?i)(?:xox[abposr]-[a-zA-Z0-9-]{10,60})",
    r"(?i)(?:sk_live_[a-zA-Z0-9]{24,})",
    r"(?i)(?:glpat-[A-Za-z0-9_-]{20,})",
    r"(?i)(?:npm_[A-Za-z0-9]{36})",
    r"(?i)(?:AAAA[A-Za-z0-9_-]{120,})",
    r"(?i)(?:basic\s+[A-Za-z0-9+/=]{20,})",
    r#"(?i)(?:"auth"\s*:\s*"[A-Za-z0-9+/=]{20,}")"#,
    r"(?i)(?:-----BEGIN\s(?:RSA\s|EC\s|DSA\s|OPENSSH\s)?PRIVATE\sKEY-----[A-Za-z0-9+/=\n\s]+-----END\s(?:RSA\s|EC\s|DSA\s|OPENSSH\s)?PRIVATE\sKEY-----)",
    r"(?i)(?:sk-[a-zA-Z0-9]{20,})",
    r"(?i)(?:eyJ[a-zA-Z0-9_-]{10,}\.eyJ[a-zA-Z0-9_-]{10,})",
    r"(?i)(?:ssh-(?:rsa|ed25519|dss)\s+[A-Za-z0-9+/=]{40,})",
    r"(?i)(?:AccountKey=[A-Za-z0-9+/]{86}[AEIMQUYcgkosw048]=)",
    r#"(?i)"private_key"\s*:\s*"-----BEGIN\s(?:RSA\s|EC\s|DSA\s|OPENSSH\s)?PRIVATE\sKEY-----[^"]*""#,
    r"(?i)[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
    r"\+\d{1,3}[\s.-]?\(?\d{2,4}\)?[\s.-]?\d{3,4}[\s.-]?\d{3,4}",
    r"\b(?:4\d{3}|5[1-5]\d{2}|2[2-7]\d{2}|3[47]\d{2}|62\d{2}|65\d{2}|6011)(?:[ -]?\d){9,12}\b",
    r"(?i)(?:\b(?:api[_-]?key|auth[_-]?token|access[_-]?token|client[_-]?secret|private[_-]?key|secret|token|password|passwd)\b\s*[:=]\s*)([A-Za-z0-9_\-\.]{20,})",
];
const EXTRACTION: &[&str] = &[
    r"(?i)(?:i\s+like|i\s+prefer|i\s+love)\s+([^.,;!]+)",
    r"(?i)(?:my\s+favorite)\s+([^.,;!]+)",
    r"(?i)(?:ich\s+mag|ich\s+bevorzuge|ich\s+liebe)\s+([^.,;!]+)",
    r"(?i)(?:mein\s+favorit\s+ist|mein\s+lieblings[^ ]+\s+ist)\s+([^.,;!]+)",
    r"(?i)(?:i\s+work\s+on|i'm\s+working\s+on|i\s+am\s+building)\s+([^.,;!]+)",
    r"(?i)(?:my\s+project\s+is|our\s+project\s+is)\s+([^.,;!]+)",
    r"(?i)(?:ich\s+arbeite\s+an|ich\s+baue\s+gerade|ich\s+bin\s+dabei)\s+([^.,;!]+)",
    r"(?i)(?:mein\s+projekt\s+ist|unsre\s+projekt\s+ist)\s+([^.,;!]+)",
    r"(?i)(?:i\s+am\s+a|i\s+work\s+as\s+a|i'm\s+a)\s+([^.,;!]+)",
    r"(?i)(?:i\s+use|i'm\s+using)\s+([^.,;!]+)",
    r"(?i)(?:ich\s+bin\s+ein|ich\s+arbeite\s+als|ich\s+bin\s+eine)\s+([^.,;!]+)",
    r"(?i)(?:ich\s+nutze|ich\s+verwende)\s+([^.,;!]+)",
];

/// Checks ASCII input against every frozen Go PII regex. Refuses all numeric
/// card candidates and secret assignments, including those Go would retain
/// after its checksum or entropy test. Regex compilation failure fails closed.
#[must_use]
pub fn direct_text_supported(text: &str) -> bool {
    static PATTERNS: OnceLock<Option<Vec<Regex>>> = OnceLock::new();
    text.is_ascii() && clear(text, &PATTERNS, PII)
}

/// Refuses potential secondary-fact patterns until their full pipeline is native.
#[must_use]
pub fn direct_content_supported(text: &str) -> bool {
    static PATTERNS: OnceLock<Option<Vec<Regex>>> = OnceLock::new();
    const KEYWORDS: &[&str] = &[
        "symaira",
        "vault",
        "memory",
        "typescript",
        "python",
        "golang",
        "sqlite",
        "project",
        "projekt",
        "preference",
        "bevorzuge",
        "lieblings",
        "arbeit",
        "work",
        "build",
        "baue",
        "agent",
        "mcp",
        "config",
        "token",
        "database",
    ];
    let lower = text.to_ascii_lowercase();
    direct_text_supported(text)
        && !KEYWORDS.iter().any(|word| lower.contains(word))
        && clear(text, &PATTERNS, EXTRACTION)
}

fn clear(text: &str, cache: &OnceLock<Option<Vec<Regex>>>, sources: &[&str]) -> bool {
    cache
        .get_or_init(|| {
            sources
                .iter()
                .map(|pattern| Regex::new(pattern))
                .collect::<Result<Vec<_>, _>>()
                .ok()
        })
        .as_ref()
        .is_some_and(|patterns| !patterns.iter().any(|pattern| pattern.is_match(text)))
}
