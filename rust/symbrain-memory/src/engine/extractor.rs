//! Ordered conversation rules with the existing grounded-evidence owner.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::evidence::{Extraction, align};

use super::text::{lower, split_sentences, trim};

/// One distinct assertion in original sentence/rule order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedFact {
    pub content: String,
    pub category: String,
    pub metadata: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Extraction>,
}

struct Rule {
    regex: Regex,
    category: &'static str,
    template: &'static str,
}

/// The complete frozen English/German pattern and keyword extractor.
pub struct PatternExtractor {
    rules: Vec<Rule>,
}

impl PatternExtractor {
    /// Compiles the fixed rules, using Go's ASCII Perl whitespace class.
    ///
    /// # Errors
    /// Returns a regex error if an authored rule is invalid.
    pub fn new() -> Result<Self, regex::Error> {
        let definitions = [
            (
                r"(?i)(?:i\s+like|i\s+prefer|i\s+love)\s+([^.,;!]+)",
                "preference",
                "User prefers %s",
            ),
            (
                r"(?i)(?:my\s+favorite)\s+([^.,;!]+)",
                "preference",
                "User's favorite is %s",
            ),
            (
                r"(?i)(?:ich\s+mag|ich\s+bevorzuge|ich\s+liebe)\s+([^.,;!]+)",
                "preference",
                "User prefers %s",
            ),
            (
                r"(?i)(?:mein\s+favorit\s+ist|mein\s+lieblings[^ ]+\s+ist)\s+([^.,;!]+)",
                "preference",
                "User's favorite is %s",
            ),
            (
                r"(?i)(?:i\s+work\s+on|i'm\s+working\s+on|i\s+am\s+building)\s+([^.,;!]+)",
                "project",
                "User is building %s",
            ),
            (
                r"(?i)(?:my\s+project\s+is|our\s+project\s+is)\s+([^.,;!]+)",
                "project",
                "User's active project is %s",
            ),
            (
                r"(?i)(?:ich\s+arbeite\s+an|ich\s+baue\s+gerade|ich\s+bin\s+dabei)\s+([^.,;!]+)",
                "project",
                "User is building %s",
            ),
            (
                r"(?i)(?:mein\s+projekt\s+ist|unsre\s+projekt\s+ist)\s+([^.,;!]+)",
                "project",
                "User's active project is %s",
            ),
            (
                r"(?i)(?:i\s+am\s+a|i\s+work\s+as\s+a|i'm\s+a)\s+([^.,;!]+)",
                "identity",
                "User is a %s",
            ),
            (
                r"(?i)(?:i\s+use|i'm\s+using)\s+([^.,;!]+)",
                "identity",
                "User uses %s",
            ),
            (
                r"(?i)(?:ich\s+bin\s+ein|ich\s+arbeite\s+als|ich\s+bin\s+eine)\s+([^.,;!]+)",
                "identity",
                "User is a %s",
            ),
            (
                r"(?i)(?:ich\s+nutze|ich\s+verwende)\s+([^.,;!]+)",
                "identity",
                "User uses %s",
            ),
        ];
        let rules = definitions
            .into_iter()
            .map(|(pattern, category, template)| {
                Regex::new(&pattern.replace(r"\s", r"[\t\n\f\r ]")).map(|regex| Rule {
                    regex,
                    category,
                    template,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { rules })
    }

    /// Extracts distinct facts with evidence aligned against the original text.
    /// The caller assigns evidence source identities before persistence.
    #[must_use]
    pub fn extract(&self, text: &str) -> Vec<ExtractedFact> {
        let mut facts = Vec::new();
        let mut seen = BTreeSet::new();
        for sentence in split_sentences(text, true) {
            let sentence = trim(sentence);
            if sentence.len() < 5 {
                continue;
            }
            let mut matched = false;
            for rule in &self.rules {
                if let Some(captured) = rule
                    .regex
                    .captures(sentence)
                    .and_then(|captures| captures.get(1))
                {
                    let captured = clean_captured(captured.as_str());
                    if captured.len() > 1 {
                        let content = rule.template.replacen("%s", captured, 1);
                        if seen.insert(content.clone()) {
                            facts.push(fact(
                                text,
                                sentence,
                                content,
                                rule.category,
                                "regex_pattern",
                            ));
                        }
                        matched = true;
                    }
                }
            }
            if !matched && high_value(sentence) {
                let content = clean_sentence(sentence);
                if content.len() > 10 && seen.insert(content.clone()) {
                    facts.push(fact(text, sentence, content, "general", "keyword_filter"));
                }
            }
        }
        facts
    }
}

fn fact(
    source: &str,
    trigger: &str,
    content: String,
    category: &str,
    method: &str,
) -> ExtractedFact {
    let (span, status) = align(source, trigger);
    ExtractedFact {
        evidence: vec![Extraction {
            text: content.clone(),
            evidence_text: trigger.into(),
            span,
            alignment_status: status.into(),
            ..Extraction::default()
        }],
        content,
        category: category.into(),
        metadata: BTreeMap::from([
            ("raw_trigger".into(), trigger.into()),
            ("method".into(), method.into()),
        ]),
    }
}

fn clean_captured(text: &str) -> &str {
    let mut text = trim(text).trim_matches(['.', '"', '\'', ';', '!', '?', ',']);
    for filler in [
        "gerade",
        "jetzt",
        "momentan",
        "basically",
        "mostly",
        "always",
        "immer",
    ] {
        if lower(text).starts_with(&format!("{filler} ")) {
            text = &text[filler.len() + 1..];
        }
    }
    trim(text)
}

fn high_value(text: &str) -> bool {
    let text = lower(text);
    [
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
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

fn clean_sentence(text: &str) -> String {
    let text = trim(text).trim_matches(['*', '-', '_', '#', ' ']);
    let replacements = [
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
    // strings.NewReplacer applies the first matching key in one pass; replacing
    // repeatedly would reinterpret generated text and alter overlapping keys.
    let mut remaining = text;
    let mut output = String::new();
    while !remaining.is_empty() {
        if let Some((from, to)) = replacements
            .iter()
            .find(|(from, _)| remaining.starts_with(from))
        {
            output.push_str(to);
            remaining = &remaining[from.len()..];
        } else if let Some(character) = remaining.chars().next() {
            output.push(character);
            remaining = &remaining[character.len_utf8()..];
        }
    }
    output
}
