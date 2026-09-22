//! Pins the request layer and the provider-level error texts against the
//! recording made by `scripts/usage-request-oracle`.
//!
//! The differential suite cannot reach this layer (a real fetch needs a live
//! endpoint and a working credential), so the shipped implementation is frozen
//! into `tests/fixtures/usage_requests.json` and this test rebuilds the same
//! fixture state in the port.

use super::super::{hostname, platform_label, request_for};
use crate::providers::Provider;
use crate::transport::{Cancellation, FixtureTransport, Response};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const ORACLE: &str = include_str!("../tests/fixtures/usage_requests.json");

const CLAUDE_ADMIN: &str = "dump-claude-admin";
const CLAUDE_OAUTH: &str = "dump-claude-oauth";
const CODEX_OAUTH: &str = "dump-codex-oauth";
const COPILOT_OAUTH: &str = "dump-copilot-oauth";
const CURSOR_COOKIE: &str = "dump-cursor-cookie";
const KIMI_CLI: &str = "dump-kimi-api-key";
const KIMI_WEB: &str = "dump-kimi-web-token";
const KIMI_DEVICE: &str = "dump-kimi-device-id";
const MOONSHOT_KEY: &str = "dump-moonshot-key";
const NOUS_TOKEN: &str = "dump-nous-token";
const OPENCODE_COOKIE: &str = "dump-opencode-cookie";
const OPENROUTER_KEY: &str = "dump-openrouter-key";

const CREDENTIALS: [&str; 11] = [
    CLAUDE_ADMIN,
    CLAUDE_OAUTH,
    CODEX_OAUTH,
    COPILOT_OAUTH,
    CURSOR_COOKIE,
    KIMI_CLI,
    KIMI_WEB,
    MOONSHOT_KEY,
    NOUS_TOKEN,
    OPENCODE_COOKIE,
    OPENROUTER_KEY,
];

/// Headers the shipped implementation fills with values that carry no
/// reproducible fact (see the note in `provider_requests_tests.rs`).
const UNPINNED_HEADERS: [&str; 3] = [
    "X-Msh-Os-Version",
    "X-Msh-Device-Model",
    "X-Server-Instance",
];

/// One strategy of the oracle fixture: which provider, which source, which
/// credential, and - for the Kimi CLI strategy - the stored device id.
struct Case {
    provider: &'static str,
    /// The source the oracle recorded (the shipped `Strategy.Source()`).
    source: &'static str,
    credential: &'static str,
    device_id: Option<&'static str>,
}

const CASES: [Case; 12] = [
    Case {
        provider: "claude",
        source: "api",
        credential: CLAUDE_ADMIN,
        device_id: None,
    },
    Case {
        provider: "claude",
        source: "oauth",
        credential: CLAUDE_OAUTH,
        device_id: None,
    },
    Case {
        provider: "codex",
        source: "oauth",
        credential: CODEX_OAUTH,
        device_id: None,
    },
    Case {
        provider: "copilot",
        source: "api",
        credential: COPILOT_OAUTH,
        device_id: None,
    },
    Case {
        provider: "cursor",
        source: "web",
        credential: CURSOR_COOKIE,
        device_id: None,
    },
    Case {
        provider: "kimi",
        source: "cli",
        credential: KIMI_CLI,
        device_id: Some(KIMI_DEVICE),
    },
    Case {
        provider: "kimi",
        source: "web",
        credential: KIMI_WEB,
        device_id: None,
    },
    Case {
        provider: "moonshot",
        source: "api",
        credential: MOONSHOT_KEY,
        device_id: None,
    },
    Case {
        provider: "nous",
        source: "api",
        credential: NOUS_TOKEN,
        device_id: None,
    },
    Case {
        provider: "opencode",
        source: "web",
        credential: OPENCODE_COOKIE,
        device_id: None,
    },
    Case {
        provider: "openrouter",
        source: "api",
        credential: OPENROUTER_KEY,
        device_id: None,
    },
    Case {
        provider: "antigravity",
        source: "local",
        credential: "",
        device_id: None,
    },
];

fn provider_for(case: &Case) -> Provider {
    let mut provider = Provider::fixture(case.provider, case.provider);
    provider.credentials = vec![(case.source.to_string(), case.credential.to_string())];
    provider.credential = Some(case.credential.to_string());
    provider.device_id = case.device_id.map(str::to_string);
    provider
}

/// Mirrors the normalization the oracle applies, so both sides compare on the
/// same placeholders.
fn normalize(value: &str) -> String {
    let mut text = value.replace(&hostname(), "<host>");
    // The Kimi identity headers carry the host platform, which differs between
    // the machine that regenerated the fixture and the one checking it.
    text = text.replace(platform_label(), "<platform>");
    for credential in CREDENTIALS {
        text = text.replace(credential, "CREDENTIAL");
    }
    text
}

fn headers_of(value: &Value) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for header in value["headers"].as_array().expect("headers array") {
        // Header names are case-insensitive; the shipped code lets Go's
        // `http.Header.Set` canonicalize them, the port keeps the source
        // spelling, so both sides compare lowercased names.
        let name = header["name"]
            .as_str()
            .expect("header name")
            .to_ascii_lowercase();
        let value = normalize(header["value"].as_str().expect("header value"));
        headers.insert(name, value);
    }
    headers
}

#[test]
fn requests_match_the_shipped_oracle_recording() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("parse usage request oracle");
    let entries = oracle["requests"].as_array().expect("requests array");
    assert_eq!(entries.len(), CASES.len(), "oracle case count changed");

    for case in &CASES {
        let entry = entries
            .iter()
            .find(|entry| entry["provider"] == case.provider && entry["source"] == case.source)
            .unwrap_or_else(|| panic!("oracle has no {}/{} entry", case.provider, case.source));
        if case.provider == "antigravity" {
            assert!(
                entry["requests"].as_array().is_some_and(Vec::is_empty),
                "{}: shipped implementation sends no request in this state",
                case.provider
            );
            continue;
        }
        let provider = provider_for(case);
        if case.provider == "opencode" {
            // The shipped OpenCode strategy walks GET workspace -> POST
            // workspace against the recorder's `200 {}` answers; replay the
            // whole executed sequence, not just its first request.
            replay_opencode_walk(case, entry, &provider);
            continue;
        }
        let request = request_for(&provider, case.source, case.credential, None);
        let shipped = entry["requests"]
            .as_array()
            .expect("requests")
            .first()
            .unwrap_or_else(|| panic!("{}/{}: no captured request", case.provider, case.source));

        assert_eq!(
            request.method, shipped["method"],
            "{} method",
            case.provider
        );
        assert_eq!(
            request.url,
            normalize(shipped["url"].as_str().expect("url")),
            "{} url",
            case.provider
        );
        let body = request.body.as_ref().map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
        assert_eq!(
            body,
            shipped["body"].as_str().unwrap_or_default(),
            "{} body",
            case.provider
        );

        let mut expected = headers_of(shipped);
        for name in UNPINNED_HEADERS {
            let shipped_value = expected.remove(&name.to_ascii_lowercase());
            let ported = request.headers.get(name);
            if name == "X-Server-Instance" {
                assert_eq!(
                    ported.map(|value| value.starts_with("server-fn:")),
                    shipped_value.map(|value| value.starts_with("server-fn:")),
                    "{}: {name} shape",
                    case.provider
                );
            } else {
                assert_eq!(ported, None, "{}: {name} is not pinned", case.provider);
                if case.provider == "kimi" && case.source == "cli" {
                    assert!(
                        shipped_value.is_some(),
                        "{}: {name} vanished",
                        case.provider
                    );
                } else {
                    assert_eq!(
                        shipped_value, None,
                        "{}: {name} is not in the recording",
                        case.provider
                    );
                }
            }
        }
        let mut ported: BTreeMap<String, String> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), normalize(value)))
            .collect();
        for name in UNPINNED_HEADERS {
            ported.remove(&name.to_ascii_lowercase());
        }
        assert_eq!(ported, expected, "{} headers", case.provider);
    }
}

/// Replays the shipped `OpenCode` strategy's full walk: the recorder answers
/// every request with `200 {}`, so the shipped code GETs the workspaces
/// server, POSTs the `[]` fallback, and fails with `missing workspace id`.
/// The port must execute exactly that sequence. The Go recorder captures the
/// strategy error directly; `Provider::fetch` adds the chain wrapper that the
/// chain runner would add in production, hence the explicit prefix.
fn replay_opencode_walk(case: &Case, entry: &Value, provider: &Provider) {
    let mut provider = provider.clone();
    provider.fixture = false;
    let answer = Ok(Response {
        status: 200,
        body: b"{}".to_vec(),
        headers: BTreeMap::new(),
    });
    let fixture = FixtureTransport::with_sequences(
        [("opencode".to_string(), vec![answer.clone(), answer])]
            .into_iter()
            .collect(),
    );
    let transport: Arc<dyn crate::transport::Transport> = Arc::new(fixture.clone());
    let cancel = Cancellation::with_timeout(Duration::from_secs(5));
    let result = provider.fetch(&transport, &cancel);

    let recorded = entry["requests"]
        .as_array()
        .unwrap_or_else(|| panic!("{}/{}: requests", case.provider, case.source));
    let executed = fixture.requests();
    assert_eq!(
        executed.len(),
        recorded.len(),
        "{}: executed request count",
        case.provider
    );
    for (index, request) in executed.iter().enumerate() {
        let shipped = &recorded[index];
        assert_eq!(
            request.method, shipped["method"],
            "{}[{index}]: method",
            case.provider
        );
        assert_eq!(
            request.url,
            normalize(shipped["url"].as_str().expect("url")),
            "{}[{index}]: url",
            case.provider
        );
        let body = request.body.as_ref().map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
        assert_eq!(
            body,
            shipped["body"].as_str().unwrap_or_default(),
            "{}[{index}]: body",
            case.provider
        );

        let mut expected = headers_of(shipped);
        let shipped_instance = expected.remove("x-server-instance");
        let mut ported: BTreeMap<String, String> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), normalize(value)))
            .collect();
        let ported_instance = ported.remove("x-server-instance");
        assert_eq!(
            ported_instance.map(|value| value.starts_with("server-fn:")),
            shipped_instance.map(|value| value.starts_with("server-fn:")),
            "{}[{index}]: X-Server-Instance shape",
            case.provider
        );
        assert_eq!(ported, expected, "{}[{index}]: headers", case.provider);
    }

    let error = result
        .expect_err("the recorder's `200 {}` answer carries no workspace id")
        .to_string();
    let strategy_error = entry["error"]
        .as_str()
        .unwrap_or_else(|| panic!("{}/{}: error", case.provider, case.source));
    assert_eq!(
        error,
        format!("all AI usage fallbacks failed: {strategy_error}"),
        "{}: recorded strategy error",
        case.provider
    );
}

#[test]
fn provider_error_texts_match_the_shipped_oracle_recording() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("parse usage request oracle");
    let chains = oracle["chains"].as_array().expect("chains array");

    // Antigravity resolves a local process, which a fixture cannot pin; its text
    // depends on whether the app runs on the machine executing this test.
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|case| case.provider != "antigravity" && case.source != "none")
        .collect();
    let providers: Vec<&str> = vec![
        "claude",
        "codex",
        "copilot",
        "cursor",
        "kimi",
        "moonshot",
        "nous",
        "opencode",
        "openrouter",
    ];
    let statuses: [(&str, u16); 4] = [
        ("unauthorized", 401),
        ("rate-limited", 429),
        ("server-error", 500),
        ("malformed", 200),
    ];

    for provider_id in providers {
        let credentials: Vec<(String, String)> = cases
            .iter()
            .filter(|case| case.provider == provider_id)
            .map(|case| (case.source.to_string(), case.credential.to_string()))
            .collect();
        let mut fixture = Provider::fixture(provider_id, provider_id);
        // The fixture branch performs a single request; the shipped provider
        // walks its whole credential chain, which is what the oracle recorded.
        fixture.fixture = false;
        fixture.credentials = credentials;
        fixture.credential = Some(
            cases
                .iter()
                .find(|case| case.provider == provider_id)
                .map_or(String::new(), |case| case.credential.to_string()),
        );
        fixture.device_id = Some(KIMI_DEVICE.to_string());

        for (label, status) in statuses {
            let body = if label == "malformed" {
                "not json at all".to_string()
            } else {
                "{\"error\":\"probe\"}".to_string()
            };
            let transport: Arc<dyn crate::transport::Transport> = Arc::new(FixtureTransport::new(
                [(
                    provider_id.to_string(),
                    Response {
                        status,
                        body: body.into_bytes(),
                        headers: BTreeMap::new(),
                    },
                )]
                .into_iter()
                .collect(),
            ));
            let cancel = Cancellation::with_timeout(Duration::from_secs(5));
            let error = fixture
                .fetch(&transport, &cancel)
                .expect_err("canned status must fail");
            let expected = chains
                .iter()
                .find(|entry| entry["provider"] == provider_id && entry["status"] == label)
                .unwrap_or_else(|| panic!("oracle has no {provider_id}/{label} chain"));
            assert_eq!(
                error.to_string(),
                expected["text"].as_str().expect("chain text"),
                "{provider_id} {label}"
            );
        }
    }
}
