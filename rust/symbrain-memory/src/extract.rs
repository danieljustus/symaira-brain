//! The shipped secondary-fact extractor (`internal/memory/extractor/pattern.go`).
//!
//! Go splits the text by ranging over runes and rebuilding each sentence with
//! `string(r)`, so every invalid byte of the input becomes `U+FFFD` in the
//! sentences and therefore in the stored facts. The same holds here: the
//! extractor runs on the rune form of the input ([`GoView::runes`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::evidence::{self, Extraction, Span};
use crate::gotext::GoView;
use crate::pii::go_regex;

/// One extracted assertion.
pub(crate) struct Fact {
    pub content: String,
    pub metadata: BTreeMap<String, String>,
    /// Grounded evidence; `source` is left for the caller.
    pub evidence: Vec<Extraction>,
}

struct Rule {
    regex: Regex,
    template: &'static str,
}

const RULES: &[(&str, &str)] = &[
    (
        r"(?i)(?:i\s+like|i\s+prefer|i\s+love)\s+([^.,;!]+)",
        "User prefers %s",
    ),
    (
        r"(?i)(?:my\s+favorite)\s+([^.,;!]+)",
        "User's favorite is %s",
    ),
    (
        r"(?i)(?:ich\s+mag|ich\s+bevorzuge|ich\s+liebe)\s+([^.,;!]+)",
        "User prefers %s",
    ),
    (
        r"(?i)(?:mein\s+favorit\s+ist|mein\s+lieblings[^ ]+\s+ist)\s+([^.,;!]+)",
        "User's favorite is %s",
    ),
    (
        r"(?i)(?:i\s+work\s+on|i'm\s+working\s+on|i\s+am\s+building)\s+([^.,;!]+)",
        "User is building %s",
    ),
    (
        r"(?i)(?:my\s+project\s+is|our\s+project\s+is)\s+([^.,;!]+)",
        "User's active project is %s",
    ),
    (
        r"(?i)(?:ich\s+arbeite\s+an|ich\s+baue\s+gerade|ich\s+bin\s+dabei)\s+([^.,;!]+)",
        "User is building %s",
    ),
    (
        r"(?i)(?:mein\s+projekt\s+ist|unsre\s+projekt\s+ist)\s+([^.,;!]+)",
        "User's active project is %s",
    ),
    (
        r"(?i)(?:i\s+am\s+a|i\s+work\s+as\s+a|i'm\s+a)\s+([^.,;!]+)",
        "User is a %s",
    ),
    (r"(?i)(?:i\s+use|i'm\s+using)\s+([^.,;!]+)", "User uses %s"),
    (
        r"(?i)(?:ich\s+bin\s+ein|ich\s+arbeite\s+als|ich\s+bin\s+eine)\s+([^.,;!]+)",
        "User is a %s",
    ),
    (
        r"(?i)(?:ich\s+nutze|ich\s+verwende)\s+([^.,;!]+)",
        "User uses %s",
    ),
];

fn rules() -> &'static [Rule] {
    static COMPILED: OnceLock<Vec<Rule>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        RULES
            .iter()
            .map(|(pattern, template)| Rule {
                regex: go_regex(pattern),
                template,
            })
            .collect()
    })
}

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

/// Go's `strings.ToLower` for the characters it maps one-to-one.
fn lower(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character == '\u{130}' {
                // Go maps U+0130 to a plain `i`.
                vec!['i']
            } else {
                character.to_lowercase().collect()
            }
        })
        .collect()
}

fn trim_set(text: &str, set: &str) -> String {
    text.trim_matches(|character| set.contains(character))
        .to_owned()
}

fn clean_captured(text: &str) -> String {
    let mut value = trim_set(text.trim(), ".\"';!?,");
    for filler in [
        "gerade",
        "jetzt",
        "momentan",
        "basically",
        "mostly",
        "always",
        "immer",
    ] {
        if lower(&value).starts_with(&format!("{filler} "))
            && let Some(rest) = value.get(filler.len() + 1..)
        {
            value = rest.to_owned();
        }
    }
    value.trim().to_owned()
}

const REPLACEMENTS: &[(&str, &str)] = &[
    ("ich arbeite", "User arbeitet"),
    ("ich baue", "User baut"),
    ("ich bin", "User ist"),
    ("ich bevorzuge", "User bevorzugt"),
    ("ich nutze", "User nutzt"),
    ("ich verwende", "User verwendet"),
    ("i am working", "User is working"),
    ("i'm working", "User is working"),
    ("i am building", "User is building"),
    ("i'm building", "User is building"),
    ("i am a", "User is a"),
    ("i'm a", "User is a"),
    ("i like", "User likes"),
    ("i prefer", "User prefers"),
    ("i use", "User uses"),
];

/// `strings.NewReplacer(...).Replace`: at each position the first matching
/// pair in argument order wins; matches never overlap.
fn replace_all(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    'scan: while index < text.len() {
        let rest = &text[index..];
        for (old, new) in REPLACEMENTS {
            if rest.starts_with(old) {
                out.push_str(new);
                index += old.len();
                continue 'scan;
            }
        }
        let Some(character) = rest.chars().next() else {
            break;
        };
        out.push(character);
        index += character.len_utf8();
    }
    out
}

fn clean_sentence(text: &str) -> String {
    replace_all(&trim_set(text.trim(), "*-_# "))
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if matches!(character, '.' | '?' | '!' | '\n') {
            if !current.trim().is_empty() {
                sentences.push(std::mem::take(&mut current));
            }
            current.clear();
        } else {
            current.push(character);
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

/// Aligns a sentence back to the input the way `evidencekit.Align` does on
/// raw bytes: the exact search sees the bytes, the other tiers see runes.
fn ground(view: &GoView, runes: &str, evidence_text: &str, fact: &str) -> Extraction {
    let (span, status) = if view.has_invalid() {
        if let Some(found) = evidence::align_exact(&view.text, evidence_text) {
            (map_span(view, None, found), "exact")
        } else if let Some(found) = evidence::align_normalized(runes, evidence_text) {
            (map_span(view, Some(runes), found), "normalized")
        } else if let Some(found) = evidence::align_fuzzy(runes, evidence_text) {
            (map_span(view, Some(runes), found), "fuzzy")
        } else {
            (Span::default(), "unmatched")
        }
    } else {
        evidence::align(&view.text, evidence_text)
    };
    Extraction {
        text: fact.to_owned(),
        evidence_text: evidence_text.to_owned(),
        span,
        alignment_status: status.to_owned(),
        ..Extraction::default()
    }
}

/// Converts a span in the view (`runes == None`) or in the rune form to
/// offsets in the original bytes.
fn map_span(view: &GoView, runes: Option<&str>, span: Span) -> Span {
    let convert = |offset: i64| -> i64 {
        let offset = usize::try_from(offset).unwrap_or(0);
        let view_offset = match runes {
            None => offset,
            Some(runes) => {
                let index = runes[..offset.min(runes.len())].chars().count();
                view.text
                    .char_indices()
                    .nth(index)
                    .map_or(view.text.len(), |(position, _)| position)
            }
        };
        i64::try_from(view.original_offset(view_offset)).unwrap_or(0)
    };
    Span {
        start: convert(span.start),
        end: convert(span.end),
    }
}

/// `PatternExtractor.ExtractFacts`.
pub(crate) fn extract_facts(original: &[u8]) -> Vec<Fact> {
    let view = GoView::new(original);
    let text = view.runes();
    let mut facts = Vec::new();
    let mut seen = BTreeSet::new();
    for sentence in split_sentences(&text) {
        let trimmed = sentence.trim();
        if trimmed.len() < 5 {
            continue;
        }
        let mut matched = false;
        for rule in rules() {
            let Some(group) = rule
                .regex
                .captures(trimmed)
                .and_then(|captures| captures.get(1))
            else {
                continue;
            };
            let captured = clean_captured(group.as_str().trim());
            if captured.len() > 1 {
                let formatted = rule.template.replacen("%s", &captured, 1);
                if seen.insert(formatted.clone()) {
                    facts.push(Fact {
                        evidence: vec![ground(&view, &text, trimmed, &formatted)],
                        content: formatted,
                        metadata: [("raw_trigger", trimmed), ("method", "regex_pattern")]
                            .into_iter()
                            .map(|(key, value)| (key.to_owned(), value.to_owned()))
                            .collect(),
                    });
                }
                matched = true;
            }
        }
        if !matched {
            let lowered = lower(trimmed);
            if KEYWORDS.iter().any(|word| lowered.contains(word)) {
                let cleaned = clean_sentence(trimmed);
                if cleaned.len() > 10 && seen.insert(cleaned.clone()) {
                    facts.push(Fact {
                        evidence: vec![ground(&view, &text, trimmed, &cleaned)],
                        content: cleaned,
                        metadata: [("raw_trigger", trimmed), ("method", "keyword_filter")]
                            .into_iter()
                            .map(|(key, value)| (key.to_owned(), value.to_owned()))
                            .collect(),
                    });
                }
            }
        }
    }
    facts
}

/// `isDuplicateOfPrimary`.
pub(crate) fn is_duplicate_of_primary(fact: &str, raw_trigger: &str, primary: &str) -> bool {
    let norm = |value: &str| trim_set(&lower(value.trim()), ".*\"';!?");
    let fact_norm = norm(fact);
    let primary_norm = norm(primary);
    if fact_norm.is_empty() {
        return false;
    }
    if fact_norm == primary_norm || primary_norm.contains(&fact_norm) {
        return true;
    }
    if !raw_trigger.is_empty() {
        return norm(raw_trigger) == primary_norm;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_and_tools_become_facts() {
        let facts = extract_facts(b"I like tea. I use vim");
        let contents: Vec<_> = facts.iter().map(|fact| fact.content.as_str()).collect();
        assert_eq!(contents, ["User prefers tea", "User uses vim"]);
        assert_eq!(facts[0].evidence[0].alignment_status, "exact");
        assert_eq!(facts[0].evidence[0].evidence_text, "I like tea");
    }

    #[test]
    fn keyword_sentences_are_rewritten() {
        let facts = extract_facts(b"I prefer Python for scripting");
        assert_eq!(facts[0].content, "User prefers Python for scripting");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters() {
        let facts = extract_facts(b"I like t\xffa");
        assert_eq!(facts[0].content, "User prefers t\u{fffd}a");
    }
}
