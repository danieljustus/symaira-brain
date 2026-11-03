//! The optional LLM verdict tier of the write-path conflict check
//! (`internal/memory/conflict/llm.go` over `internal/memory/llm/client.go`).

use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};
use symaira_core_llm::{ChatOptions, ClientBuilder, GenerateOption, Message, lookup};

use crate::conflict::{Pair, Verdict};
use crate::pii::{UNTRUSTED_PREAMBLE, go_regex, sanitize_untrusted};
use crate::slog::quote_bytes;

const DEFAULT_OLLAMA_BASE: &str = "http://localhost:11434";
const DEFAULT_OLLAMA_MODEL: &str = "llama3";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

const SYSTEM_PROMPT: &str = "You are a memory-store conflict resolver. For each pair of facts you receive, decide whether the new fact (from the current write) REPEATS the old fact (same claim, possibly reworded), CONTRADICTS it (same subject, incompatible claim), or is AMBIGUOUS (related but not clearly either). Answer with a JSON object: {\"verdicts\":[{\"pair\":N,\"verdict\":\"repeat|contradiction|ambiguous\"}]} — one entry per pair, in order. When in doubt, answer \"ambiguous\": a silently wrong supersession is worse than a visible conflict. Never invent pairs that were not given.";

/// The configured endpoint of the verdict tier.
#[derive(Clone, Debug, Default)]
pub struct LlmSettings {
    pub provider: String,
    pub url: String,
    pub model: String,
}

fn schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {"verdicts": {"type": "array", "items": {
            "type": "object",
            "properties": {
                "pair": {"type": "integer"},
                "verdict": {"type": "string", "enum": ["repeat", "contradiction", "ambiguous"]}
            },
            "required": ["pair", "verdict"]
        }}},
        "required": ["verdicts"]
    })
}

/// `ollamaRoot`: the scheme and host of an endpoint, without any path.
fn ollama_root(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'/' && index > "http://".len() {
            return &raw[..index];
        }
    }
    raw
}

/// `LLMVerdictProvider.Verdicts`.
pub(crate) fn verdicts(settings: &LlmSettings, pairs: &[Pair]) -> Result<Vec<Verdict>, String> {
    let mut prompt: Vec<u8> = Vec::new();
    for (index, pair) in pairs.iter().enumerate() {
        prompt.extend_from_slice(format!("[pair {index}]\nnew fact (written now, by ").as_bytes());
        prompt.extend_from_slice(quote_bytes(&pair.new_actor).as_bytes());
        prompt.extend_from_slice(b"): ");
        prompt.extend_from_slice(&sanitize_untrusted(&pair.new_content));
        prompt.extend_from_slice(b"\nold fact (already stored, by ");
        prompt.extend_from_slice(quote_bytes(&pair.old_actor).as_bytes());
        prompt.extend_from_slice(b"): ");
        prompt.extend_from_slice(&sanitize_untrusted(&pair.old_content));
        prompt.extend_from_slice(b"\n\n");
    }
    let prompt = String::from_utf8_lossy(&prompt).into_owned();
    let system = format!("{SYSTEM_PROMPT}\n\n{UNTRUSTED_PREAMBLE}");

    let provider = if settings.provider.is_empty() {
        if std::env::var_os("OPENAI_API_KEY").is_some_and(|value| !value.is_empty()) {
            "openai"
        } else {
            "ollama"
        }
    } else {
        settings.provider.as_str()
    };
    let raw = if provider == "openai" {
        query_openai(&system, &prompt)?
    } else {
        query_ollama(settings, &system, &prompt)?
    };
    Ok(parse_response(&raw, pairs.len()))
}

fn query_ollama(settings: &LlmSettings, system: &str, prompt: &str) -> Result<String, String> {
    let wrap =
        |error: String| format!("conflict: llm verdict query: failed to query ollama: {error}");
    let descriptor = lookup("ollama")
        .ok_or_else(|| wrap("llm: ollama descriptor missing from embedded registry".into()))?;
    let base = if settings.url.is_empty() {
        DEFAULT_OLLAMA_BASE
    } else {
        settings.url.as_str()
    };
    let model = if settings.model.is_empty() {
        DEFAULT_OLLAMA_MODEL
    } else {
        settings.model.as_str()
    };
    // Building the client fails before the "failed to query ollama" wrapper
    // in the shipped code, so only the transport call is wrapped there.
    let client = ClientBuilder::new(descriptor.clone(), "")
        .base_url(ollama_root(base))
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .map_err(|error| format!("conflict: llm verdict query: {error}"))?;
    let mut out = String::new();
    client
        .generate(
            model,
            prompt,
            &GenerateOption {
                system: Some(system.to_owned()),
                format: Some(schema()),
                ..GenerateOption::default()
            },
            |chunk| {
                out.push_str(&chunk.response);
                Ok(())
            },
        )
        .map_err(|error| wrap(error.to_string()))?;
    Ok(out)
}

fn query_openai(system: &str, prompt: &str) -> Result<String, String> {
    let descriptor = lookup("openai").ok_or_else(|| {
        "conflict: llm verdict query: llm: openai descriptor missing from embedded registry"
            .to_owned()
    })?;
    // The shipped client never copies the configured URL or model into its
    // OpenAI fields, so the descriptor's endpoint and the default model apply.
    let client = ClientBuilder::new(descriptor.clone(), "")
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .map_err(|error| format!("conflict: llm verdict query: {error}"))?;
    let choice = client
        .chat(
            DEFAULT_OPENAI_MODEL,
            &[Message {
                role: "user".into(),
                content: prompt.to_owned(),
            }],
            Some(&ChatOptions {
                system: system.to_owned(),
                response_format: Some(schema()),
                ..ChatOptions::default()
            }),
        )
        .map_err(|error| format!("conflict: llm verdict query: failed to query openai: {error}"))?;
    Ok(choice.content)
}

fn word(text: &str) -> Verdict {
    match text.trim().to_lowercase().as_str() {
        "repeat" => Verdict::Repeat,
        "contradiction" => Verdict::Contradiction,
        _ => Verdict::Ambiguous,
    }
}

/// `parseVerdictResponse`: strict JSON first, then token salvage; the result
/// always has `want` entries.
pub(crate) fn parse_response(raw: &str, want: usize) -> Vec<Verdict> {
    if want == 0 {
        return Vec::new();
    }
    let mut out = vec![Verdict::Ambiguous; want];
    if let Some(entries) = strict(raw) {
        for (pair, verdict) in entries {
            if let Ok(index) = usize::try_from(pair)
                && index < want
            {
                out[index] = word(&verdict);
            }
        }
        return out;
    }
    static TOKEN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let token =
        TOKEN.get_or_init(|| go_regex(r#"(?i)"verdict"\s*:\s*"(repeat|contradiction|ambiguous)""#));
    for (index, found) in token.captures_iter(raw).enumerate() {
        if index >= want {
            break;
        }
        out[index] = match found[1].to_lowercase().as_str() {
            "repeat" => Verdict::Repeat,
            "contradiction" => Verdict::Contradiction,
            _ => Verdict::Ambiguous,
        };
    }
    out
}

/// Decodes into `{"verdicts":[{"pair":int,"verdict":string}]}` the way
/// `json.Unmarshal` does; any syntax or type error, or an empty list, is
/// `None` (Go falls back to the salvage path).
fn strict(raw: &str) -> Option<Vec<(i64, String)>> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let object = value.as_object()?;
    let mut entries: Option<&Vec<Value>> = None;
    for (key, member) in object {
        if key.eq_ignore_ascii_case("verdicts") {
            match member {
                Value::Array(list) => entries = Some(list),
                Value::Null => entries = None,
                _ => return None,
            }
        }
    }
    let entries = entries?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let (mut pair, mut verdict) = (0_i64, String::new());
        match entry {
            Value::Null => {}
            Value::Object(fields) => {
                for (key, field) in fields {
                    if key.eq_ignore_ascii_case("pair") {
                        match field {
                            Value::Null => {}
                            Value::Number(number) => pair = number.as_i64()?,
                            _ => return None,
                        }
                    } else if key.eq_ignore_ascii_case("verdict") {
                        match field {
                            Value::Null => {}
                            Value::String(text) => verdict.clone_from(text),
                            _ => return None,
                        }
                    }
                }
            }
            _ => return None,
        }
        out.push((pair, verdict));
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_json_assigns_by_pair_index() {
        let parsed = parse_response(
            r#"{"verdicts":[{"pair":1,"verdict":"Contradiction"},{"pair":7,"verdict":"repeat"}]}"#,
            2,
        );
        assert_eq!(parsed, [Verdict::Ambiguous, Verdict::Contradiction]);
    }

    #[test]
    fn malformed_output_is_salvaged_in_order() {
        let parsed = parse_response(
            r#"nope "verdict": "repeat" and "VERDICT":"contradiction""#,
            3,
        );
        assert_eq!(
            parsed,
            [Verdict::Repeat, Verdict::Contradiction, Verdict::Ambiguous]
        );
    }

    #[test]
    fn a_type_error_falls_back_to_salvage() {
        let parsed = parse_response(r#"{"verdicts":[{"pair":"0","verdict":"repeat"}]}"#, 1);
        assert_eq!(parsed, [Verdict::Repeat]);
    }
}
