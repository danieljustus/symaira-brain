//! Pins the request layer and the provider-level error texts against the
//! recording made by `scripts/usage-request-oracle`.
//!
//! The differential suite cannot reach this layer (a real fetch needs a live
//! endpoint and a working credential), so the shipped implementation is frozen
//! into `tests/fixtures/usage_requests.json` and this test rebuilds the same
//! fixture state in the port.

use super::super::{hostname, platform_label, request_for};
use crate::providers::Provider;
use crate::providers::{
    claude_from_resolved, codex_from_resolved, copilot_from_resolved, cursor_from_resolved,
    kimi_from_resolved, moonshot_from_resolved, nous_from_resolved, opencode_from_resolved,
    openrouter_from_resolved, parse_claude_keychain_blob,
};
use crate::transport::{Cancellation, FixtureTransport, Response};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const ORACLE: &str = include_str!("../tests/fixtures/usage_requests.json");
const CLAUDE_OAUTH_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/claude_oauth_authenticated_report.json");
const CLAUDE_KEYCHAIN_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/claude_keychain_authenticated_report.json");
const CODEX_FILE_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/codex_file_authenticated_report.json");
const COPILOT_FILE_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/copilot_file_authenticated_report.json");
const COMBINED_NATIVE_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/combined_native_authenticated_report.json");
const KIMI_FALLBACK_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/kimi_fallback_authenticated_report.json");
const NOUS_PRECEDENCE_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/nous_env_file_precedence_report.json");
const NOUS_FILE_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/nous_file_authenticated_report.json");
const ANTIGRAVITY_REPORT_ORACLE: &str =
    include_str!("../tests/fixtures/antigravity_authenticated_report.json");

const CLAUDE_ADMIN: &str = "dump-claude-admin";
const CLAUDE_OAUTH: &str = "dump-claude-oauth";
const CODEX_OAUTH: &str = "dump-codex-oauth";
const COPILOT_OAUTH: &str = "dump-copilot-oauth";
const REPORT_ENV_CREDENTIAL: &str = "synthetic-direct-env-credential";
const REPORT_FILE_CREDENTIAL: &str = "oracle-only-invalid-claude-oauth-file";
const CODEX_FILE_CREDENTIAL: &str = "oracle-only-invalid-codex-file";
const COPILOT_FILE_CREDENTIAL: &str = "oracle-only-invalid-copilot-file";
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
    authenticated_direct_provider_report(response, "copilot")
}

fn authenticated_claude_admin_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "claude")
}

fn authenticated_claude_oauth_report(
    response: Response,
    source: &str,
    credential: &str,
) -> (crate::Report, FixtureTransport) {
    let provider = claude_from_resolved(
        None,
        None,
        Some((source.into(), credential.into())),
        None,
        None,
    );
    let transport = FixtureTransport::new([("claude".into(), response)].into());
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    (report, transport)
}

fn authenticated_codex_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "codex")
}

fn authenticated_codex_file_report(response: Response) -> (crate::Report, FixtureTransport) {
    let oracle: Value =
        serde_json::from_str(CODEX_FILE_REPORT_ORACLE).expect("Go Codex file-authenticated report");
    let providers = oracle["success"]["providers"]
        .as_array()
        .expect("Go provider rows")
        .iter()
        .map(|row| {
            let id = row["id"].as_str().expect("provider id");
            if id == "codex" {
                return codex_from_resolved(
                    Some(("file".into(), CODEX_FILE_CREDENTIAL.into())),
                    None,
                    true,
                );
            }
            let name = row["display_name"].as_str().expect("provider display name");
            let mut provider = Provider::fixture(id, name);
            provider.configured = row["configured"].as_bool().expect("configured state");
            provider.auth_status =
                serde_json::from_value(row["auth_status"].clone()).expect("Go auth status");
            provider.credential = None;
            provider.credentials.clear();
            provider
        })
        .collect();
    let transport = FixtureTransport::new([("codex".into(), response)].into());
    let report = crate::Service::with_transport(providers, Arc::new(transport.clone())).report();
    (report, transport)
}

fn authenticated_copilot_file_report(response: Response) -> (crate::Report, FixtureTransport) {
    let provider =
        copilot_from_resolved(Some(("file".into(), COPILOT_FILE_CREDENTIAL.into())), None);
    let transport = FixtureTransport::new([("copilot".into(), response)].into());
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    (report, transport)
}

fn authenticated_openrouter_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "openrouter")
}

fn authenticated_moonshot_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "moonshot")
}

fn authenticated_cursor_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "cursor")
}

fn authenticated_kimi_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "kimi")
}

fn authenticated_nous_report(response: Response) -> (crate::Report, FixtureTransport) {
    authenticated_direct_provider_report(response, "nous")
}

fn authenticated_nous_file_report(response: Response) -> (crate::Report, FixtureTransport) {
    let parser_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/nous_file_token_oracle.json"
    ))
    .expect("Go Nous auth.json parser oracle");
    let token = parser_oracle["cases"]
        .as_array()
        .expect("Nous parser cases")
        .iter()
        .find(|case| case["id"] == "canonical-future-jwt")
        .and_then(|case| case["token"].as_str())
        .expect("Go selected a canonical live JWT");
    let provider = nous_from_resolved(
        Some(("file".into(), token.into())),
        None,
        "https://portal.nousresearch.com".into(),
    );
    let transport = FixtureTransport::new([("nous".into(), response)].into());
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    (report, transport)
}

fn authenticated_direct_provider_report(
    response: Response,
    configured_provider: &str,
) -> (crate::Report, FixtureTransport) {
    let credential = Some(("env".into(), REPORT_ENV_CREDENTIAL.into()));
    let provider = match configured_provider {
        "claude" => claude_from_resolved(credential, None, None, None, None),
        "codex" => codex_from_resolved(credential, None, false),
        "copilot" => copilot_from_resolved(credential, None),
        "cursor" => cursor_from_resolved(credential, None),
        "kimi" => kimi_from_resolved(
            credential,
            None,
            None,
            None,
            None,
            "https://api.kimi.com".into(),
            None,
        ),
        "moonshot" => moonshot_from_resolved(credential, None, "ai"),
        "nous" => nous_from_resolved(credential, None, "https://portal.nousresearch.com".into()),
        "openrouter" => {
            openrouter_from_resolved(credential, None, "https://openrouter.ai/api/v1".into())
        }
        _ => panic!("unsupported direct provider {configured_provider}"),
    };
    let transport = FixtureTransport::new([(configured_provider.into(), response)].into());
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    (report, transport)
}

fn go_report_provider_row(report: &Value, provider_id: &str) -> Value {
    report["providers"]
        .as_array()
        .expect("Go provider rows")
        .iter()
        .find(|row| row["id"] == provider_id)
        .unwrap_or_else(|| panic!("Go report has no {provider_id} row"))
        .clone()
}

fn assert_single_provider_report_matches_go(
    report: &crate::Report,
    provider_id: &str,
    expected: &Value,
) {
    assert_eq!(
        report.providers.len(),
        1,
        "only {provider_id} is under test"
    );
    assert_eq!(report.providers[0].id, provider_id);
    assert_eq!(
        serde_json::to_value(&report.providers[0]).expect("Rust provider report row"),
        *expected,
        "{provider_id} provider report row"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn antigravity_local_probe_report_and_requests_match_production_go_oracle() {
    let oracle: Value = serde_json::from_str(ANTIGRAVITY_REPORT_ORACLE)
        .expect("Go Antigravity local report oracle");
    let cases = oracle["cases"]
        .as_array()
        .expect("Antigravity oracle cases");
    assert_eq!(cases.len(), 5);

    for case in cases {
        let case_id = case["id"].as_str().expect("case id");
        let process_list = case["process_list"]
            .as_str()
            .expect("synthetic process list");
        let provider = crate::providers::provider_config::antigravity_from_process_list(
            (!process_list.is_empty()).then_some(process_list),
        );
        let expected_responses = case["responses"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|response| {
                let headers = response["headers"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            value.as_str().expect("response header value").to_string(),
                        )
                    })
                    .collect();
                Ok(Response {
                    status: u16::try_from(response["status"].as_u64().expect("response status"))
                        .expect("HTTP status fits u16"),
                    body: response["body"]
                        .as_str()
                        .expect("response body")
                        .as_bytes()
                        .to_vec(),
                    headers,
                })
            })
            .collect::<Vec<Result<Response, String>>>();
        let transport =
            FixtureTransport::with_sequences([("antigravity".into(), expected_responses)].into());
        let transport_ref: Arc<dyn crate::Transport> = Arc::new(transport.clone());
        let port_outputs = case["ports_by_pid"].as_object();
        let result = crate::providers::wrap_fetch_error(
            &provider.id,
            crate::providers::fetch_antigravity_from_observations(
                &provider,
                &transport_ref,
                &Cancellation::new(),
                process_list,
                |pid| {
                    port_outputs
                        .and_then(|outputs| outputs.get(&pid.to_string()))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                },
            ),
        );
        let mut actual_row = crate::ProviderUsage::from_provider(&provider);
        match result {
            Ok(snapshot) => actual_row.snapshot = Some(snapshot),
            Err(error) => actual_row.error = Some(error.to_string()),
        }

        let expected_report: crate::Report =
            serde_json::from_value(case["report"].clone()).expect("Go report fixture");
        let expected_row = expected_report.providers.first().expect("Go provider row");
        if let (Some(actual), Some(expected)) =
            (actual_row.snapshot.as_mut(), expected_row.snapshot.as_ref())
        {
            // Only fetched_at is wall-clock data. Resets and all parsed values
            // remain direct comparisons against the Go production report.
            actual.fetched_at = expected.fetched_at;
        }
        assert_eq!(
            serde_json::to_value(&actual_row).expect("Rust Antigravity provider row"),
            serde_json::to_value(expected_row).expect("Go Antigravity provider row"),
            "Antigravity report case {case_id}"
        );

        let expected_requests = case["requests"].as_array().expect("Go request trace");
        let actual_requests = transport.requests();
        assert_eq!(
            actual_requests.len(),
            expected_requests.len(),
            "{case_id} request count"
        );
        let port_numbers: Vec<String> = case["ports_by_pid"]
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(_, output)| {
                output
                    .as_str()
                    .unwrap_or_default()
                    .split(':')
                    .skip(1)
                    .filter_map(|tail| tail.split_whitespace().next())
                    .filter(|port| port.chars().all(|ch| ch.is_ascii_digit()))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        for (actual, expected) in actual_requests.iter().zip(expected_requests) {
            let mut url = actual.url.clone();
            for port in &port_numbers {
                url = url.replace(&format!("127.0.0.1:{port}"), "127.0.0.1:<port>");
            }
            let headers = actual
                .headers
                .iter()
                .filter(|(name, _)| {
                    [
                        "accept",
                        "content-type",
                        "connect-protocol-version",
                        "x-codeium-csrf-token",
                    ]
                    .iter()
                    .any(|allowed| name.eq_ignore_ascii_case(allowed))
                })
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            let expected_headers = expected["headers"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
                .collect::<serde_json::Map<_, _>>();
            let normalized = serde_json::json!({
                "method": actual.method,
                "url": url,
                "headers": headers,
                "body": actual.body.as_deref().map(String::from_utf8_lossy).map(std::borrow::Cow::into_owned).unwrap_or_default(),
            });
            let expected_normalized = serde_json::json!({
                "method": expected["method"],
                "url": expected["url"],
                "headers": expected_headers,
                "body": expected["body"].as_str().unwrap_or_default(),
            });
            assert_eq!(normalized, expected_normalized, "{case_id} request");
        }
    }
}

fn authenticated_opencode_report(responses: Vec<Response>) -> (crate::Report, FixtureTransport) {
    let provider = opencode_from_resolved(
        Some(("env".into(), REPORT_ENV_CREDENTIAL.into())),
        None,
        None,
    );
    let transport = FixtureTransport::with_sequences(
        [("opencode".into(), responses.into_iter().map(Ok).collect())].into(),
    );
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    (report, transport)
}

#[test]
#[allow(clippy::too_many_lines)]
fn combined_native_providers_match_the_production_go_report_and_overrides() {
    let expected: Value = serde_json::from_str(COMBINED_NATIVE_REPORT_ORACLE)
        .expect("Go combined configured-provider report");
    let kimi = kimi_from_resolved(
        Some(("env".into(), "combined-kimi-api-token".into())),
        None,
        Some("combined-kimi-cli-token"),
        None,
        None,
        "https://api.kimi.com/custom/v1".into(),
        Some("combined-device-id".into()),
    );
    let moonshot = moonshot_from_resolved(
        Some(("env".into(), "combined-moonshot-token".into())),
        None,
        "cn",
    );
    let nous = nous_from_resolved(
        Some(("file".into(), "combined-nous-file-token".into())),
        None,
        "https://portal.nousresearch.com".into(),
    );
    let opencode = opencode_from_resolved(
        Some(("env".into(), "combined-opencode-cookie".into())),
        None,
        Some("wrk_combined123"),
    );
    let openrouter = openrouter_from_resolved(
        Some(("env".into(), "combined-openrouter-token".into())),
        None,
        "https://openrouter.ai/custom/v1".into(),
    );
    let transport = FixtureTransport::new(
        [
            (
                "kimi".into(),
                Response {
                    status: 200,
                    body: include_bytes!(
                        "../../../internal/usage/testdata/kimi-fallback-api-usages.json"
                    )
                    .to_vec(),
                    headers: BTreeMap::new(),
                },
            ),
            (
                "moonshot".into(),
                Response {
                    status: 200,
                    body: include_bytes!(
                        "../../../internal/usage/testdata/moonshot-balance-cn.json"
                    )
                    .to_vec(),
                    headers: BTreeMap::new(),
                },
            ),
            (
                "nous".into(),
                Response {
                    status: 200,
                    body: include_bytes!("../../../internal/usage/testdata/nous-account.json")
                        .to_vec(),
                    headers: BTreeMap::new(),
                },
            ),
            (
                "opencode".into(),
                Response {
                    status: 200,
                    body: include_bytes!(
                        "../../../internal/usage/testdata/opencode-subscription-no-reset.json"
                    )
                    .to_vec(),
                    headers: BTreeMap::new(),
                },
            ),
            (
                "openrouter".into(),
                Response {
                    status: 200,
                    body: include_bytes!(
                        "../../../internal/usage/testdata/openrouter-credits.json"
                    )
                    .to_vec(),
                    headers: BTreeMap::new(),
                },
            ),
        ]
        .into(),
    );
    let mut report = crate::Service::with_transport(
        vec![kimi, moonshot, nous, opencode, openrouter],
        Arc::new(transport.clone()),
    )
    .report();
    assert_eq!(
        report
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["kimi", "moonshot", "nous", "opencode", "openrouter"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 5);
    let find_request = |matches: &dyn Fn(&crate::transport::Request) -> bool| {
        requests.iter().find(|request| matches(request))
    };
    let kimi_request =
        find_request(&|request| request.url == "https://api.kimi.com/custom/v1/coding/v1/usages")
            .expect("Kimi API request");
    assert_eq!(
        kimi_request
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer combined-kimi-api-token")
    );
    assert!(
        find_request(&|request| request.url == "https://api.moonshot.cn/v1/users/me/balance")
            .is_some()
    );
    assert!(
        find_request(
            &|request| request.headers.get("Authorization").map(String::as_str)
                == Some("Bearer combined-nous-file-token")
        )
        .is_some()
    );
    assert!(
        find_request(&|request| {
            request.url.starts_with("https://opencode.ai/_server?")
                && request.url.contains("args=%5B%22wrk_combined123%22%5D")
        })
        .is_some()
    );
    assert!(
        find_request(&|request| request.url == "https://openrouter.ai/custom/v1/auth/key")
            .is_some()
    );

    let go_rows = expected["providers"].as_array().expect("Go provider rows");
    for provider in &mut report.providers {
        let go_row = go_rows
            .iter()
            .find(|row| row["id"] == provider.id)
            .unwrap_or_else(|| panic!("Go report has no {} row", provider.id));
        let mut rust_row = serde_json::to_value(&*provider).expect("Rust provider row");
        let fetched_at = go_row["snapshot"]["fetched_at"]
            .as_str()
            .expect("Go canonical fetched_at");
        rust_row["snapshot"]["fetched_at"] = Value::String(fetched_at.to_owned());
        assert_eq!(rust_row, *go_row, "{} provider row", provider.id);
    }
}

#[test]
fn nous_environment_credential_precedes_the_default_file() {
    let expected: Value =
        serde_json::from_str(NOUS_PRECEDENCE_REPORT_ORACLE).expect("Go Nous env-over-file report");
    let provider = nous_from_resolved(
        Some(("env".into(), "nous-env-precedence-token".into())),
        None,
        "https://portal.nousresearch.com".into(),
    );
    let transport = FixtureTransport::new(
        [(
            "nous".into(),
            Response {
                status: 200,
                body: include_bytes!("../../../internal/usage/testdata/nous-account.json").to_vec(),
                headers: BTreeMap::new(),
            },
        )]
        .into(),
    );
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    assert_eq!(
        report.providers[0].auth_status.source.as_deref(),
        Some("env")
    );
    assert_eq!(
        transport.requests()[0]
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer nous-env-precedence-token")
    );
    let expected_row = &expected["providers"][0];
    let mut rust_row = serde_json::to_value(&report.providers[0]).expect("Rust Nous row");
    rust_row["snapshot"]["fetched_at"] = expected_row["snapshot"]["fetched_at"].clone();
    assert_eq!(rust_row, *expected_row);
}

#[test]
#[allow(clippy::too_many_lines)]
fn kimi_cli_and_web_fallback_reports_match_production_go_chains() {
    let expected: Value =
        serde_json::from_str(KIMI_FALLBACK_REPORT_ORACLE).expect("Go Kimi fallback reports");
    let api_response = Response {
        status: 401,
        body: br#"{"error":"unauthorized"}"#.to_vec(),
        headers: BTreeMap::new(),
    };
    let cli_success = Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/kimi-fallback-api-usages.json")
            .to_vec(),
        headers: BTreeMap::new(),
    };
    let cli_transport = FixtureTransport::with_sequences(
        [(
            "kimi".into(),
            vec![Ok(api_response.clone()), Ok(cli_success)],
        )]
        .into(),
    );
    let cli = kimi_from_resolved(
        Some(("env".into(), "fallback-kimi-api-token".into())),
        None,
        Some("fallback-kimi-cli-token"),
        None,
        None,
        "https://api.kimi.com".into(),
        Some("fallback-device-id".into()),
    );
    let cli_report =
        crate::Service::with_transport(vec![cli], Arc::new(cli_transport.clone())).report();
    assert_eq!(
        cli_report.providers[0].snapshot.as_ref().unwrap().source,
        "cli"
    );
    let cli_requests = cli_transport.requests();
    assert_eq!(cli_requests.len(), 2);
    assert_eq!(
        cli_requests[0]
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer fallback-kimi-api-token")
    );
    assert_eq!(
        cli_requests[1]
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer fallback-kimi-cli-token")
    );
    assert_eq!(
        cli_requests[1]
            .headers
            .get("X-Msh-Device-Id")
            .map(String::as_str),
        Some("fallback-device-id")
    );
    let cli_expected = &expected["api_fails_cli_recovers"]["providers"][0];
    let mut cli_row = serde_json::to_value(&cli_report.providers[0]).expect("Rust Kimi CLI row");
    cli_row["snapshot"]["fetched_at"] = cli_expected["snapshot"]["fetched_at"].clone();
    assert_eq!(cli_row, *cli_expected);

    let web_success = Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/kimi-fallback-web-usages.json")
            .to_vec(),
        headers: BTreeMap::new(),
    };
    let web_transport = FixtureTransport::with_sequences(
        [(
            "kimi".into(),
            vec![Ok(api_response.clone()), Ok(api_response), Ok(web_success)],
        )]
        .into(),
    );
    let web = kimi_from_resolved(
        Some(("env".into(), "fallback-kimi-api-token".into())),
        None,
        Some("fallback-kimi-cli-token"),
        Some(("env".into(), "fallback-kimi-web-token".into())),
        None,
        "https://api.kimi.com".into(),
        Some("fallback-device-id".into()),
    );
    let web_report =
        crate::Service::with_transport(vec![web], Arc::new(web_transport.clone())).report();
    assert_eq!(
        web_report.providers[0].snapshot.as_ref().unwrap().source,
        "web"
    );
    let web_requests = web_transport.requests();
    assert_eq!(web_requests.len(), 3);
    assert_eq!(
        web_requests[2].url,
        "https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages"
    );
    assert_eq!(
        web_requests[2]
            .headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer fallback-kimi-web-token")
    );
    let web_expected = &expected["api_cli_fail_web_works"]["providers"][0];
    let mut web_row = serde_json::to_value(&web_report.providers[0]).expect("Rust Kimi web row");
    web_row["snapshot"]["fetched_at"] = web_expected["snapshot"]["fetched_at"].clone();
    assert_eq!(web_row, *web_expected);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
    assert_single_provider_report_matches_go(
        &report,
        "copilot",
        &go_report_provider_row(&report_oracle, "copilot"),
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
            report.providers[0].error.as_deref(),
            Some(
                copilot["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
    }
}

#[test]
fn authenticated_claude_admin_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/claude_admin_authenticated_report.json"
    ))
    .expect("Go authenticated Claude Admin report");
    let (mut report, transport) = authenticated_claude_admin_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/claude-admin-cost.json").to_vec(),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
    assert_eq!(usage.id, "claude");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Admin API key from ANTHROPIC_ADMIN_KEY"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Claude snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][0]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][0]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://api.anthropic.com/v1/organizations/cost_report?bucket_width=1d&limit=7"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_single_provider_report_matches_go(
        &report,
        "claude",
        &go_report_provider_row(&report_oracle, "claude"),
    );

    // The Admin API emits a USD snapshot even when there are no totals.
    let (empty_report, _) = authenticated_claude_admin_report(Response {
        status: 200,
        body: b"{}".to_vec(),
        headers: BTreeMap::new(),
    });
    let empty = empty_report.providers[0]
        .snapshot
        .as_ref()
        .expect("empty Claude Admin response still yields a snapshot");
    assert!(empty.meters.is_empty());
    assert_eq!(empty.currency.as_deref(), Some("USD"));

    let go_oracle: Value = serde_json::from_str(ORACLE).expect("Go request oracle");
    let chains = go_oracle["chains"].as_array().expect("error chains");
    for (status, label, body) in [
        (401, "unauthorized", br#"{"error":"nope"}"#.as_slice()),
        (429, "rate-limited", br#"{"error":"slow down"}"#.as_slice()),
        (200, "malformed", b"not-json".as_slice()),
    ] {
        let expected_chain = chains
            .iter()
            .find(|entry| entry["provider"] == "claude" && entry["status"] == label)
            .and_then(|entry| entry["text"].as_str())
            .unwrap_or_else(|| panic!("Go oracle has no Claude/{label} error"));
        let expected = expected_chain
            .split_once("; ")
            .map_or(expected_chain, |(first, _)| first);
        let (report, _) = authenticated_claude_admin_report(Response {
            status,
            body: body.to_vec(),
            headers: BTreeMap::new(),
        });
        assert_eq!(
            report.providers[0].error.as_deref(),
            Some(expected),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
    }
}

#[test]
fn authenticated_claude_oauth_report_matches_go_build_report_oracle() {
    let oracle: Value =
        serde_json::from_str(CLAUDE_OAUTH_REPORT_ORACLE).expect("Go Claude OAuth report oracle");
    assert_claude_oauth_report_matches_scenario(
        &oracle,
        "success",
        "env",
        REPORT_ENV_CREDENTIAL,
        "errors",
    );
}

#[test]
fn authenticated_claude_oauth_file_report_matches_go_build_report_oracle() {
    let oracle: Value =
        serde_json::from_str(CLAUDE_OAUTH_REPORT_ORACLE).expect("Go Claude OAuth report oracle");
    assert_claude_oauth_report_matches_scenario(
        &oracle,
        "file_success",
        "file",
        REPORT_FILE_CREDENTIAL,
        "file_errors",
    );
}

#[test]
fn authenticated_claude_keychain_report_matches_go_production_builder() {
    let oracle: Value = serde_json::from_str(CLAUDE_KEYCHAIN_REPORT_ORACLE)
        .expect("Go Claude keychain report oracle");
    let credential_blob = oracle["credential_blob"]
        .as_str()
        .expect("synthetic keychain blob")
        .as_bytes();
    let (credential, expires_at) = parse_claude_keychain_blob(credential_blob)
        .expect("Rust decodes the same synthetic keychain blob");

    let mut scenarios = vec![&oracle["success"]];
    scenarios.extend(
        oracle["errors"]
            .as_array()
            .expect("Go keychain report errors")
            .iter(),
    );
    assert_eq!(scenarios.len(), 4);
    for scenario in scenarios {
        let status = u16::try_from(scenario["status"].as_u64().expect("HTTP status"))
            .expect("status fits u16");
        let body = scenario["body"].as_str().expect("canned response body");
        let response_headers = scenario["response_headers"]
            .as_object()
            .expect("Go canned response headers")
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    value.as_str().expect("response header value").to_string(),
                )
            })
            .collect();
        let response = Response {
            status,
            body: body.as_bytes().to_vec(),
            headers: response_headers,
        };
        let provider = claude_from_resolved(
            None,
            None,
            Some(("keychain".into(), credential.clone())),
            None,
            expires_at,
        );
        let transport = FixtureTransport::new([("claude".into(), response)].into());
        let report =
            crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
        let requests = transport.requests();
        assert_eq!(requests.len(), 1, "status {status}");
        let request = &requests[0];
        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://api.anthropic.com/api/oauth/usage");
        let authorization = format!("Bearer {credential}");
        assert_eq!(
            request.headers.get("Authorization").map(String::as_str),
            Some(authorization.as_str())
        );
        assert_eq!(
            request.headers.get("anthropic-beta").map(String::as_str),
            Some("oauth-2025-04-20")
        );

        let expected = &scenario["report"];
        let mut actual = serde_json::to_value(&report).expect("Rust keychain report");
        if expected["providers"][0]["snapshot"].is_object() {
            actual["providers"][0]["snapshot"]["fetched_at"] =
                expected["providers"][0]["snapshot"]["fetched_at"].clone();
        }
        assert_eq!(actual, *expected, "Claude keychain report status {status}");

        let expected_request = &scenario["requests"][0];
        assert_eq!(expected_request["method"], request.method.as_str());
        assert_eq!(expected_request["url"], request.url.as_str());
        for (header, value) in expected_request["headers"]
            .as_object()
            .expect("Go safe headers")
        {
            assert_eq!(
                request.headers.get(header).map(String::as_str),
                value.as_str(),
                "header {header}"
            );
        }
    }
}

fn assert_claude_oauth_report_matches_scenario(
    oracle: &Value,
    scenario: &str,
    source: &str,
    credential: &str,
    errors_key: &str,
) {
    let success = &oracle[scenario];
    let (report, transport) = authenticated_claude_oauth_report(
        Response {
            status: 200,
            body: include_bytes!("../../../internal/usage/testdata/claude-oauth-usage.json")
                .to_vec(),
            headers: BTreeMap::new(),
        },
        source,
        credential,
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "https://api.anthropic.com/api/oauth/usage");
    let expected_authorization = format!("Bearer {credential}");
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some(expected_authorization.as_str())
    );
    assert_eq!(
        requests[0]
            .headers
            .get("anthropic-beta")
            .map(String::as_str),
        Some("oauth-2025-04-20")
    );

    let mut actual = serde_json::to_value(&report).expect("Rust Claude OAuth report");
    actual["providers"][0]["snapshot"]["fetched_at"] =
        success["providers"][0]["snapshot"]["fetched_at"].clone();
    assert_eq!(actual, *success);

    let errors = oracle[errors_key]
        .as_array()
        .expect("Go report error cases");
    assert_eq!(errors.len(), 3);
    for (index, (status, body, retry_after)) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17")),
        (200, b"not-json".as_slice(), None),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(errors[index]["status"], status);
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, transport) = authenticated_claude_oauth_report(
            Response {
                status,
                body: body.to_vec(),
                headers,
            },
            source,
            credential,
        );
        assert_eq!(transport.requests().len(), 1);
        assert_eq!(
            serde_json::to_value(report).expect("Rust Claude OAuth error report"),
            errors[index]["report"]
        );
    }
}

#[test]
fn authenticated_codex_report_matches_go_success_and_failure_oracles() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/codex_authenticated_report.json"
    ))
    .expect("Go authenticated Codex report");
    let oracle: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let codex = oracle["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "codex")
        .expect("Codex oracle case");

    let (mut report, transport) = authenticated_codex_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/codex-wham-usage.json").to_vec(),
        headers: BTreeMap::new(),
    });
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
    assert_eq!(usage.id, "codex");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Signed in via Codex CLI OAuth (CODEX_ACCESS_TOKEN or auth.json)"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = usage.snapshot.as_mut().expect("Codex snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][1]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(snapshot).expect("Rust snapshot"),
        report_oracle["providers"][1]["snapshot"]
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://chatgpt.com/backend-api/wham/usage"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer synthetic-direct-env-credential")
    );
    assert_single_provider_report_matches_go(
        &report,
        "codex",
        &go_report_provider_row(&report_oracle, "codex"),
    );

    for (status, body, retry_after, oracle_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), None, 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), Some("17"), 1),
        (200, b"{}".as_slice(), None, 2),
    ] {
        let headers = retry_after.map_or_else(BTreeMap::new, |value| {
            [("Retry-After".into(), value.into())].into_iter().collect()
        });
        let (report, _) = authenticated_codex_report(Response {
            status,
            body: body.to_vec(),
            headers,
        });
        assert_eq!(
            report.providers[0].error.as_deref(),
            Some(
                codex["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
    assert_single_provider_report_matches_go(
        &report,
        "openrouter",
        &go_report_provider_row(&report_oracle, "openrouter"),
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
            report.providers[0].error.as_deref(),
            Some(
                openrouter["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
    assert_single_provider_report_matches_go(
        &report,
        "moonshot",
        &go_report_provider_row(&report_oracle, "moonshot"),
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
            report.providers[0].error.as_deref(),
            Some(
                moonshot["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
    assert_single_provider_report_matches_go(
        &report,
        "cursor",
        &go_report_provider_row(&report_oracle, "cursor"),
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
            report.providers[0].error.as_deref(),
            Some(
                cursor["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
                    .fixed_offset()
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
            .fixed_offset(),
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
    let mut rust_row = serde_json::to_value(&report.providers[0]).expect("Rust Kimi row");
    let mut go_row = go_report_provider_row(&report_oracle, "kimi");
    normalize_rfc3339_fields(&mut rust_row);
    normalize_rfc3339_fields(&mut go_row);
    assert_eq!(rust_row, go_row, "Kimi provider report row");

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
            report.providers[0].error.as_deref(),
            Some(
                kimi["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
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
    assert_eq!(report.providers.len(), 1);
    let usage = &mut report.providers[0];
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
    assert_single_provider_report_matches_go(
        &report,
        "nous",
        &go_report_provider_row(&report_oracle, "nous"),
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
            report.providers[0].error.as_deref(),
            Some(
                nous["errors"][oracle_index]["text"]
                    .as_str()
                    .expect("Go error text")
            ),
            "HTTP {status} report error"
        );
        assert_eq!(report.providers[0].snapshot, None);
    }
}

#[test]
fn authenticated_nous_live_jwt_file_matches_go_production_constructor() {
    let oracle: Value =
        serde_json::from_str(NOUS_FILE_REPORT_ORACLE).expect("Go Nous file-authenticated reports");
    for (response, expected) in [
        (
            Response {
                status: 200,
                body: include_bytes!("../../../internal/usage/testdata/nous-account.json").to_vec(),
                headers: BTreeMap::new(),
            },
            &oracle["success"],
        ),
        (
            Response {
                status: 401,
                body: br#"{"error":"nope"}"#.to_vec(),
                headers: BTreeMap::new(),
            },
            &oracle["errors"][0]["report"],
        ),
        (
            Response {
                status: 429,
                body: br#"{"error":"slow down"}"#.to_vec(),
                headers: BTreeMap::new(),
            },
            &oracle["errors"][1]["report"],
        ),
        (
            Response {
                status: 200,
                body: b"{}".to_vec(),
                headers: BTreeMap::new(),
            },
            &oracle["errors"][2]["report"],
        ),
    ] {
        let (report, transport) = authenticated_nous_file_report(response);
        assert_eq!(report.providers.len(), 1);
        assert_eq!(
            report.providers[0].auth_status.source.as_deref(),
            Some("file")
        );
        assert_eq!(transport.requests().len(), 1);
        assert_eq!(
            transport.requests()[0]
                .headers
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer header.eyJleHAiOjQxMDI0NDQ4MDB9.signature")
        );
        let mut rust = serde_json::to_value(report).expect("Rust Nous file report");
        if expected["providers"][0]["snapshot"].is_object() {
            rust["providers"][0]["snapshot"]["fetched_at"] =
                expected["providers"][0]["snapshot"]["fetched_at"].clone();
        }
        assert_eq!(rust, *expected);
    }
}

#[test]
fn authenticated_opencode_report_matches_go_success_oracle_and_workspace_walk() {
    let report_oracle: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/opencode_authenticated_report.json"
    ))
    .expect("Go authenticated OpenCode report");
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/provider_cases.json"))
        .expect("Go provider cases");
    let subscription = cases["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == "opencode")
        .expect("OpenCode oracle case");
    let (mut report, transport) = authenticated_opencode_report(vec![
        Response {
            status: 200,
            body: br#"{"workspaces":[{"id":"wrk_abc123def"}]}"#.to_vec(),
            headers: BTreeMap::new(),
        },
        Response {
            status: 200,
            body: serde_json::to_vec(&subscription["response"]).expect("response fixture"),
            headers: BTreeMap::new(),
        },
    ]);
    assert_eq!(report.providers.len(), 1);
    let usage = &report.providers[0];
    assert_eq!(usage.id, "opencode");
    assert!(usage.configured);
    assert_eq!(usage.auth_status.status, "available");
    assert_eq!(
        usage.auth_status.detail,
        "Cookie configured (OPENCODE_COOKIE)"
    );
    assert_eq!(usage.auth_status.source.as_deref(), Some("env"));
    assert_eq!(usage.error, None);
    let snapshot = report.providers[0]
        .snapshot
        .as_mut()
        .expect("OpenCode snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        report_oracle["providers"][7]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle timestamp"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    for (meter, expected) in snapshot.meters.iter_mut().zip(
        report_oracle["providers"][7]["snapshot"]["meters"]
            .as_array()
            .unwrap(),
    ) {
        meter.resets_at = Some(
            chrono::DateTime::parse_from_rfc3339(
                expected["resets_at"]
                    .as_str()
                    .expect("oracle reset timestamp"),
            )
            .expect("oracle reset timestamp parses"),
        );
    }

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert!(
        requests[0]
            .url
            .contains("id=def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f")
    );
    assert_eq!(requests[1].method, "GET");
    assert!(requests[1].url.contains("args=%5B%22wrk_abc123def%22%5D"));
    assert!(
        requests[1]
            .url
            .contains("id=7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4")
    );
    for request in &requests {
        assert_eq!(
            request.headers.get("Cookie").map(String::as_str),
            Some(REPORT_ENV_CREDENTIAL)
        );
    }

    let mut rust_row = serde_json::to_value(&report.providers[0]).expect("Rust OpenCode row");
    let mut go_row = go_report_provider_row(&report_oracle, "opencode");
    normalize_rfc3339_fields(&mut rust_row);
    normalize_rfc3339_fields(&mut go_row);
    assert_eq!(rust_row, go_row, "OpenCode provider report row");
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

#[test]
fn authenticated_codex_file_report_matches_go_success_and_failure_oracles() {
    let oracle: Value =
        serde_json::from_str(CODEX_FILE_REPORT_ORACLE).expect("Go Codex file report oracle");
    let (mut report, transport) = authenticated_codex_file_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/codex-wham-usage.json").to_vec(),
        headers: BTreeMap::new(),
    });
    let codex = &mut report.providers[1];
    assert_eq!(codex.id, "codex");
    assert!(codex.configured);
    assert_eq!(codex.auth_status.status, "available");
    assert_eq!(codex.auth_status.source.as_deref(), Some("file"));
    assert_eq!(codex.error, None);
    let snapshot = codex.snapshot.as_mut().expect("Codex file snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        oracle["success"]["providers"][1]["snapshot"]["fetched_at"]
            .as_str()
            .expect("oracle fetched_at"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Codex file report"),
        oracle["success"]
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].url,
        "https://chatgpt.com/backend-api/wham/usage"
    );
    assert_eq!(
        requests[0].headers.get("Authorization").map(String::as_str),
        Some("Bearer oracle-only-invalid-codex-file")
    );

    for (status, body, error_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), 1),
        (200, b"{}".as_slice(), 2),
    ] {
        let (report, _) = authenticated_codex_file_report(Response {
            status,
            body: body.to_vec(),
            headers: BTreeMap::new(),
        });
        assert_eq!(
            serde_json::to_value(&report).expect("Rust Codex file error report"),
            oracle["errors"][error_index]["report"],
            "HTTP {status} report"
        );
    }
}

#[test]
fn authenticated_copilot_file_report_matches_go_success_and_failure_oracles() {
    let oracle: Value =
        serde_json::from_str(COPILOT_FILE_REPORT_ORACLE).expect("Go Copilot file report oracle");
    let (mut report, transport) = authenticated_copilot_file_report(Response {
        status: 200,
        body: include_bytes!("../../../internal/usage/testdata/copilot-user.json").to_vec(),
        headers: BTreeMap::new(),
    });
    let copilot = &mut report.providers[0];
    assert_eq!(copilot.id, "copilot");
    assert!(copilot.configured);
    assert_eq!(copilot.auth_status.status, "available");
    assert_eq!(copilot.auth_status.source.as_deref(), Some("file"));
    assert_eq!(copilot.error, None);
    let snapshot = copilot.snapshot.as_mut().expect("Copilot file snapshot");
    snapshot.fetched_at = chrono::DateTime::parse_from_rfc3339(
        oracle["success"]["providers"][0]["snapshot"]["fetched_at"]
            .as_str()
            .expect("Go fetched_at"),
    )
    .expect("oracle timestamp parses")
    .with_timezone(&chrono::Utc);
    assert_eq!(
        serde_json::to_value(&report).expect("Rust Copilot file report"),
        oracle["success"]
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
        Some("Bearer oracle-only-invalid-copilot-file")
    );

    for (status, body, error_index) in [
        (401, br#"{"error":"nope"}"#.as_slice(), 0),
        (429, br#"{"error":"slow down"}"#.as_slice(), 1),
        (200, b"{}".as_slice(), 2),
    ] {
        let (report, _) = authenticated_copilot_file_report(Response {
            status,
            body: body.to_vec(),
            headers: BTreeMap::new(),
        });
        assert_eq!(
            serde_json::to_value(&report).expect("Rust Copilot file error report"),
            oracle["errors"][error_index]["report"],
            "HTTP {status} report"
        );
    }
}
