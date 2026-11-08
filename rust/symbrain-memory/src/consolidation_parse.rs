//! Tolerant parsing of the consolidation LLM reply (ported for #760 from
//! `parseJSONResponse` in `internal/memory/consolidation/engine.go`).
//!
//! Candidates are tried most-specific first: the raw reply, a fenced code
//! block, the outermost `{...}` span, then the same after stripping a
//! reasoning model's `<think>` preamble.

use regex::Regex;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Go decodes JSON `null` into the zero value.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// One synthesized fact.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ConsolidatedItem {
    /// Synthesized content.
    #[serde(default, deserialize_with = "null_default")]
    pub content: String,
    /// 1-based prompt indices (later mapped to ids) of merged memories.
    #[serde(default, deserialize_with = "null_default")]
    pub replaces_ids: Vec<String>,
    /// Free-form metadata.
    #[serde(default, deserialize_with = "null_default")]
    pub metadata: BTreeMap<String, String>,
}

/// The parsed reply.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ConsolidationResult {
    /// Synthesized facts.
    #[serde(default, deserialize_with = "null_default")]
    pub consolidated: Vec<ConsolidatedItem>,
    /// Indices the model marked transient.
    #[serde(default, deserialize_with = "null_default")]
    pub discarded_ids: Vec<String>,
}

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```[^\n`]*\n(.*?)```").expect("static regex"))
}

fn think_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<think>.*?</think>").expect("static regex"))
}

fn brace_span(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then(|| &s[start..=end])
}

/// Parses `raw` into a validated result.
///
/// # Errors
/// The Go-worded message when no candidate decodes, or when a decoded item
/// has empty content.
pub fn parse_response(raw: &str) -> Result<ConsolidationResult, String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        let s = s.trim();
        if !s.is_empty() && !candidates.iter().any(|c| c == s) {
            candidates.push(s.to_owned());
        }
    };
    let cleaned = raw.trim();
    add(cleaned);
    if let Some(m) = fence_re().captures(cleaned) {
        add(&m[1]);
    }
    if let Some(span) = brace_span(cleaned) {
        add(span);
    }
    if think_re().is_match(cleaned) {
        let stripped = think_re().replace_all(cleaned, "").trim().to_owned();
        if let Some(m) = fence_re().captures(&stripped) {
            add(&m[1]);
        }
        add(&stripped);
        if let Some(span) = brace_span(&stripped) {
            add(span);
        }
    }
    let mut last_error = String::new();
    for candidate in &candidates {
        match serde_json::from_str::<ConsolidationResult>(candidate) {
            Err(error) => last_error = error.to_string(),
            Ok(result) => {
                for (i, item) in result.consolidated.iter().enumerate() {
                    if item.content.trim().is_empty() {
                        return Err(format!(
                            "consolidation result validation failed: consolidated item {i} has empty content"
                        ));
                    }
                }
                return Ok(result);
            }
        }
    }
    Err(format!(
        "failed to parse consolidation result JSON: {last_error} (raw response: {raw})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"consolidated":[{"content":"Daniel likes Rust.","replaces_ids":["1","2"],"metadata":{"topic":"prefs"}}],"discarded_ids":["3"]}"#;

    fn ok(raw: &str) -> ConsolidationResult {
        parse_response(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"))
    }

    #[test]
    fn accepted_shapes_table() {
        for raw in [
            GOOD.to_owned(),
            format!("```json\n{GOOD}\n```"),
            format!("Sure! Here you go:\n```\n{GOOD}\n```\nHope it helps"),
            format!("prefix text {GOOD} trailing words"),
            format!("<think>hmm {{not json}}</think>\n{GOOD}"),
            format!("<think>x</think>\n```json\n{GOOD}\n```"),
        ] {
            let got = ok(&raw);
            assert_eq!(got.consolidated[0].content, "Daniel likes Rust.", "{raw}");
            assert_eq!(got.consolidated[0].replaces_ids, ["1", "2"]);
            assert_eq!(got.consolidated[0].metadata["topic"], "prefs");
            assert_eq!(got.discarded_ids, ["3"]);
        }
        assert_eq!(ok("{}"), ConsolidationResult::default());
        assert_eq!(
            ok(r#"{"consolidated":null,"discarded_ids":null}"#),
            ConsolidationResult::default()
        );
    }

    #[test]
    fn rejected_shapes() {
        let empty = parse_response(r#"{"consolidated":[{"content":"  "}]}"#).unwrap_err();
        assert_eq!(
            empty,
            "consolidation result validation failed: consolidated item 0 has empty content"
        );
        let garbage = parse_response("no json here").unwrap_err();
        assert!(garbage.starts_with("failed to parse consolidation result JSON: "));
        assert!(garbage.ends_with("(raw response: no json here)"));
        assert!(parse_response("").is_err());
    }
}
