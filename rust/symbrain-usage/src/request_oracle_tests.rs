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
const REPORT_ENV_CREDENTIAL: &str = "synthetic-direct-env-credential";
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

const CASES: [Case; 13] = [
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
        source: "api",
        credential: KIMI_CLI,
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

fn authenticated_copilot_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "copilot",
        include_str!("../tests/fixtures/copilot_authenticated_report.json"),
    )
}

fn authenticated_openrouter_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "openrouter",
        include_str!("../tests/fixtures/openrouter_authenticated_report.json"),
    )
}

fn authenticated_moonshot_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "moonshot",
        include_str!("../tests/fixtures/moonshot_authenticated_report.json"),
    )
}

fn authenticated_cursor_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "cursor",
        include_str!("../tests/fixtures/cursor_authenticated_report.json"),
    )
}

fn authenticated_kimi_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "kimi",
        include_str!("../tests/fixtures/kimi_authenticated_report.json"),
    )
}

fn authenticated_nous_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(
        response,
        "nous",
        include_str!("../tests/fixtures/nous_authenticated_report.json"),
    )
}

fn authenticated_direct_provider_report(
    response: Response,
    configured_provider: &str,
    report_fixture: &str,
) -> (crate::Report, FixtureTransport) {
    // Rebuild all ten report rows from the source-pinned Go fixture. The
    // providers with no credentials have no probe state; Antigravity's
    // fixture credential is absent to select its deterministic not-running
    // result. Only the selected provider gets a synthetic direct env token.
    let oracle: Value = serde_json::from_str(report_fixture).expect("Go authenticated report");
    let providers = oracle["providers"]
        .as_array()
        .expect("Go provider rows")
        .iter()
        .map(|row| {
            let id = row["id"].as_str().expect("provider id");
            let name = row["display_name"].as_str().expect("provider name");
            let mut provider = Provider::fixture(id, name);
            provider.configured = row["configured"].as_bool().expect("configured state");
            provider.auth_status =
                serde_json::from_value(row["auth_status"].clone()).expect("Go auth status");
            provider.credential = None;
            provider.credentials.clear();
            if id == configured_provider {
                let strategy = if id == "kimi" { "api" } else { "env" };
                provider.credentials = vec![(strategy.into(), REPORT_ENV_CREDENTIAL.into())];
                provider.credential = Some(REPORT_ENV_CREDENTIAL.into());
            }
            provider
        })
        .collect();
    let transport = FixtureTransport::new([(configured_provider.into(), response)].into());
    let report = crate::Service::with_transport(providers, Arc::new(transport.clone())).report();
    (report, transport)
}

fn normalize_rfc3339_fields(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "fetched_at" | "resets_at")
                    && let Some(text) = value.as_str()
                    && let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text)
                {
                    let formatted = parsed.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
                    let formatted = formatted.strip_suffix('Z').unwrap_or(&formatted);
                    let normalized = formatted.split_once('.').map_or_else(
                        || format!("{formatted}Z"),
                        |(seconds, fraction)| {
                            let fraction = fraction.trim_end_matches('0');
                            if fraction.is_empty() {
                                format!("{seconds}Z")
                            } else {
                                format!("{seconds}.{fraction}Z")
                            }
                        },
                    );
                    *value = Value::String(normalized);
                    continue;
                }
                normalize_rfc3339_fields(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_rfc3339_fields(value);
            }
        }
        _ => {}
    }
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

#[test]
fn authenticated_copilot_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/copilot_authenticated_report.json"
    ))
    .expect("Go authenticated Copilot report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let copilot = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "copilot")
        .expect("Copilot oracle case");

    let (mut report, transport) = authenticated_copilot_report(Response {
        status: 200,
        body: serde_json::to_vec(&copilot["response"]).expect("response fixture"),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["copilot", "antigravity"]
    );
    let usage = &mut report.providers[2];
    assert_eq!(usage.id, "copilot");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Signed in via GitHub Copilot (COPILOT_ACCESS_TOKEN or Copilot CLI)"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Copilot snapshot");
    // Go's source oracle canonicalizes its clock; normalize the Rust clock to
    // the same value before comparing the complete parsed snapshot.
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        copilot["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        copilot["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://api.github.com/copilot_internal/user"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Copilot report"),
        report_oracle
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_copilot_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[2].error.as_deref(),
            Some(
                copilot["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[2].snapshot, None);
    }
}

#[test]
fn authenticated_openrouter_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/openrouter_authenticated_report.json"
    ))
    .expect("Go authenticated OpenRouter report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let openrouter = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "openrouter")
        .expect("OpenRouter oracle case");

    let (mut report, transport) = authenticated_openrouter_report(Response {
        status: 200,
        body: serde_json::to_vec(&openrouter["response"]).expect("response fixture"),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["openrouter", "antigravity"]
    );
    let usage = &mut report.providers[8];
    assert_eq!(usage.id, "openrouter");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(usage.auth_status.detail, "API key from OPENROUTER_API_KEY");
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("OpenRouter snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][8]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][8]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "https://openrouter.ai/api/v1/auth/key");
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_eq!(
        requests[0].headers.get("X-Title").map(String::as_str),
        Some("symbrain")
    );
    assert_eq!(
        serde_json::to_value(&report).expect("Rust OpenRouter report"),
        report_oracle
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_openrouter_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[8].error.as_deref(),
            Some(
                openrouter["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[8].snapshot, None);
    }
}

#[test]
fn authenticated_moonshot_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/moonshot_authenticated_report.json"
    ))
    .expect("Go authenticated Moonshot report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let moonshot = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "moonshot")
        .expect("Moonshot oracle case");

    let (mut report, transport) = authenticated_moonshot_report(Response {
        status: 200,
        body: serde_json::to_vec(&moonshot["response"]).expect("response fixture"),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["moonshot", "antigravity"]
    );
    let usage = &mut report.providers[5];
    assert_eq!(usage.id, "moonshot");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(usage.auth_status.detail, "API key from MOONSHOT_API_KEY");
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Moonshot snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][5]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][5]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://api.moonshot.ai/v1/users/me/balance"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Moonshot report"),
        report_oracle
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_moonshot_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[5].error.as_deref(),
            Some(
                moonshot["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[5].snapshot, None);
    }
}

#[test]
fn authenticated_cursor_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/cursor_authenticated_report.json"
    ))
    .expect("Go authenticated Cursor report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let cursor = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "cursor")
        .expect("Cursor oracle case");

    let (mut report, transport) = authenticated_cursor_report(Response {
        status: 200,
        body: serde_json::to_vec(&cursor["response"]).expect("response fixture"),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["cursor", "antigravity"]
    );
    let usage = &mut report.providers[3];
    assert_eq!(usage.id, "cursor");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Cookie configured (CURSOR_COOKIE)"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Cursor snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][3]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][3]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "https://cursor.com/api/usage-summary");
    assert_eq!(
        requests[0].headers.get("Cookie").map(String::as_str),
        Some("synthetic-direct-env-credential")
    );
    assert_eq!(
        requests[0].headers.get("Accept").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Cursor report"),
        report_oracle
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_cursor_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[3].error.as_deref(),
            Some(
                cursor["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[3].snapshot, None);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn authenticated_kimi_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/kimi_authenticated_report.json"
    ))
    .expect("Go authenticated Kimi report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let kimi = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "kimi")
        .expect("Kimi oracle case");

    let (mut report, transport) = authenticated_kimi_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/kimi-api-usages.json").to_vec(),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["kimi", "antigravity"]
    );
    let usage = &mut report.providers[4];
    assert_eq!(usage.id, "kimi");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(usage.auth_status.detail, "API key from KIMI_CODE_API_KEY");
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Kimi snapshot");
    let response: Value = serde_json::from_slice(include_bytes!(
        "../../../internal/usage/testdata/kimi-api-usages.json"
    ))
    .expect("Go Kimi response fixture");
    for (meter, response_time) in [
        (
            &snapshot.meters[0],
            response["usage"]["resetTime"].as_str().unwrap(),
        ),
        (
            &snapshot.meters[1],
            response["limits"][0]["detail"]["resetTime"]
                .as_str()
                .unwrap(),
        ),
    ] {
        assert_eq!(
            meter.resets_at,
            Some(
                chrono::DateTime::parse_from_rfc3339(response_time)
                    .expect("Go response reset timestamp")
                    .with_timezone(&chrono::Utc)
            ),
            "{} reset timestamp follows the Go response",
            meter.label
        );
    }
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][4]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    for (meter, index) in snapshot.meters.iter_mut().zip(0..2) {
        meter.resets_at = Some(
            chrono::DateTime::parse_from_rfc3339(
                report_oracle["providers"][4]["snapshot"]["meters"][index]["resets_at"]
                    .as_str()
                    .expect("oracle reset timestamp"),
            )
            .expect("oracle reset timestamp parses")
            .with_timezone(&chrono::Utc),
        );
    }
    let mut rust_snapshot = serde_json::to_value(snapshot).expect("Rust snapshot");
    let mut go_snapshot = report_oracle["providers"][4]["snapshot"].clone();
    normalize_rfc3339_fields(&mut rust_snapshot);
    normalize_rfc3339_fields(&mut go_snapshot);
    assert_eq!(rust_snapshot, go_snapshot);
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "https://api.kimi.com/coding/v1/usages");
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_eq!(
        requests[0].headers.get("Accept").map(String::as_str),
        Some("application/json")
    );
    let mut rust_report = serde_json::to_value(&report).expect("Rust Kimi report");
    let mut go_report = report_oracle;
    normalize_rfc3339_fields(&mut rust_report);
    normalize_rfc3339_fields(&mut go_report);
    assert_eq!(rust_report, go_report);

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_kimi_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[4].error.as_deref(),
            Some(
                kimi["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[4].snapshot, None);
    }
}

#[test]
fn authenticated_nous_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/nous_authenticated_report.json"
    ))
    .expect("Go authenticated Nous report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let nous = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "nous")
        .expect("Nous oracle case");

    let (mut report, transport) = authenticated_nous_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/nous-account.json").to_vec(),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 10);
    assert_eq!(
        report
            .providers
            .iter()
            .filter(|provider| provider.configured)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["nous", "antigravity"]
    );
    let usage = &mut report.providers[6];
    assert_eq!(usage.id, "nous");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Signed in via Hermes CLI auth store (NOUS_PORTAL_ACCESS_TOKEN or file)"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Nous snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][6]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][6]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://portal.nousresearch.com/api/oauth/account"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Nous report"),
        report_oracle
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_nous_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[6].error.as_deref(),
            Some(
                nous["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[6].snapshot, None);
    }
}

#[path = "request_oracle_opencode_tests.rs"]
mod opencode_walk;
use opencode_walk::replay_opencode_walk;

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
