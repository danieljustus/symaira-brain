//! The default LLM transport of the consolidation engine (ported for #760
//! from `internal/memory/llm/client.go` at v0.12.0): native Ollama
//! `/api/generate` or an OpenAI-wire chat completion, both pinned to the
//! consolidation response schema.

use std::time::Duration;

use serde_json::{Value, json};
use symaira_core_llm::{ChatOptions, ClientBuilder, GenerateOption, Message, lookup};

use crate::conflict_llm::LlmSettings;

const DEFAULT_OLLAMA_BASE: &str = "http://localhost:11434";
const DEFAULT_OLLAMA_MODEL: &str = "llama3";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

/// JSON Schema (draft-07) of the consolidation reply.
#[must_use]
pub fn response_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "consolidated": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "content": {"type": "string"},
                    "replaces_ids": {"type": "array", "items": {"type": "string"}},
                    "metadata": {"type": "object", "additionalProperties": {"type": "string"}}
                },
                "required": ["content", "replaces_ids", "metadata"]
            }},
            "discarded_ids": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["consolidated", "discarded_ids"]
    })
}

/// Scheme and host of an endpoint without any path (`/v1` included).
fn ollama_root(raw: &str) -> &str {
    raw.bytes()
        .enumerate()
        .find(|(i, b)| *b == b'/' && *i > "http://".len())
        .map_or(raw, |(i, _)| &raw[..i])
}

/// One schema-pinned query. An empty provider auto-detects through
/// `OPENAI_API_KEY`; a zero `timeout` means 45 s.
///
/// # Errors
/// The Go-worded transport or client-construction failure.
pub fn query(
    settings: &LlmSettings,
    timeout: Duration,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let timeout = if timeout.is_zero() {
        DEFAULT_TIMEOUT
    } else {
        timeout
    };
    let provider = if settings.provider.is_empty() {
        if std::env::var_os("OPENAI_API_KEY").is_some_and(|v| !v.is_empty()) {
            "openai"
        } else {
            "ollama"
        }
    } else {
        settings.provider.as_str()
    };
    if provider == "openai" {
        query_openai(settings, timeout, system, prompt)
    } else {
        query_ollama(settings, timeout, system, prompt)
    }
}

fn query_ollama(
    settings: &LlmSettings,
    timeout: Duration,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let descriptor = lookup("ollama")
        .ok_or_else(|| "llm: ollama descriptor missing from embedded registry".to_owned())?;
    let base = if settings.url.is_empty() {
        DEFAULT_OLLAMA_BASE
    } else {
        &settings.url
    };
    let model = if settings.model.is_empty() {
        DEFAULT_OLLAMA_MODEL
    } else {
        &settings.model
    };
    let client = ClientBuilder::new(descriptor.clone(), "")
        .base_url(ollama_root(base))
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    client
        .generate(
            model,
            prompt,
            &GenerateOption {
                system: Some(system.to_owned()),
                format: Some(response_schema()),
                ..GenerateOption::default()
            },
            |chunk| {
                out.push_str(&chunk.response);
                Ok(())
            },
        )
        .map_err(|e| format!("failed to query ollama: {e}"))?;
    Ok(out)
}

fn query_openai(
    settings: &LlmSettings,
    timeout: Duration,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let descriptor = lookup("openai")
        .ok_or_else(|| "llm: openai descriptor missing from embedded registry".to_owned())?;
    let mut builder = ClientBuilder::new(descriptor.clone(), "").timeout(timeout);
    if !settings.url.is_empty() && settings.provider == "openai" {
        builder = builder.base_url(&settings.url);
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let model = if settings.model.is_empty() {
        DEFAULT_OPENAI_MODEL
    } else {
        &settings.model
    };
    let choice = client
        .chat(
            model,
            &[Message {
                role: "user".into(),
                content: prompt.to_owned(),
            }],
            Some(&ChatOptions {
                system: system.to_owned(),
                response_format: Some(response_schema()),
                ..ChatOptions::default()
            }),
        )
        .map_err(|e| format!("failed to query openai: {e}"))?;
    Ok(choice.content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn ollama_root_strips_paths() {
        for (raw, want) in [
            ("http://localhost:11434", "http://localhost:11434"),
            ("http://h:1/v1", "http://h:1"),
            ("https://h/api/x", "https://h"),
        ] {
            assert_eq!(ollama_root(raw), want);
        }
    }

    #[test]
    fn schema_requires_both_top_level_keys() {
        let schema = response_schema();
        assert_eq!(schema["required"], json!(["consolidated", "discarded_ids"]));
        assert_eq!(
            schema["properties"]["consolidated"]["items"]["required"]
                .as_array()
                .map(Vec::len),
            Some(3)
        );
    }

    /// Reads one complete HTTP request: a single `read` can return only the
    /// headers when the body arrives in a later TCP segment.
    fn read_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut data = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            data.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&data).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    return text;
                }
            }
            if n == 0 {
                return text;
            }
        }
    }

    /// Local fake Ollama: one streamed `/api/generate` exchange, no internet.
    #[test]
    fn ollama_path_streams_from_a_local_fake_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let body = "{\"response\":\"{\\\"consolidated\\\":[],\",\"model\":\"tiny\",\"created_at\":\"2026-01-01T00:00:00Z\",\"done\":false}\n{\"response\":\"\\\"discarded_ids\\\":[]}\",\"model\":\"tiny\",\"created_at\":\"2026-01-01T00:00:00Z\",\"done\":true}\n";
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            request
        });
        let settings = LlmSettings {
            provider: "ollama".into(),
            url: format!("http://127.0.0.1:{port}/v1"),
            model: "tiny".into(),
        };
        let text = query(&settings, Duration::from_secs(5), "SYS", "USER").unwrap();
        assert_eq!(text, "{\"consolidated\":[],\"discarded_ids\":[]}");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /api/generate"), "{request}");
        assert!(request.contains("\"model\":\"tiny\"") && request.contains("\"system\":\"SYS\""));
        assert!(request.contains("discarded_ids"), "schema pinned in format");
    }
}
