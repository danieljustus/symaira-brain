use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use symbrain_usage::{Provider, Response, Service, Transport};

const GO_ORACLE: &str = include_str!("fixtures/provider_graph.json");

fn fixture_service() -> Service {
    let names = [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("copilot", "GitHub Copilot"),
        ("cursor", "Cursor"),
        ("kimi", "Kimi Code"),
        ("moonshot", "Moonshot"),
        ("nous", "Nous Portal"),
        ("opencode", "OpenCode Go"),
        ("openrouter", "OpenRouter"),
        ("antigravity", "Antigravity"),
    ];
    let providers = names
        .iter()
        .map(|(id, name)| Provider::fixture(id, name))
        .collect();
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/provider_cases.json")).expect("provider cases");
    let responses = names
        .iter()
        .map(|(id, _)| {
            let case = cases["providers"]
                .as_array()
                .expect("provider cases")
                .iter()
                .find(|case| case["id"] == *id)
                .expect("case for provider");
            (
                (*id).to_string(),
                Response {
                    status: 200,
                    body: serde_json::to_vec(&case["response"]).expect("response JSON"),
                    headers: BTreeMap::new(),
                },
            )
        })
        .collect();
    Service::with_transport(
        providers,
        Arc::new(symbrain_usage::FixtureTransport::new(responses)),
    )
}

#[test]
fn fixture_report_covers_complete_provider_graph_in_contract_order() {
    let report = fixture_service().report();
    let ids: Vec<_> = report
        .providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "claude",
            "codex",
            "copilot",
            "cursor",
            "kimi",
            "moonshot",
            "nous",
            "opencode",
            "openrouter",
            "antigravity"
        ]
    );
    assert!(
        report.providers[..10]
            .iter()
            .all(|provider| provider.snapshot.is_some() && provider.error.is_none())
    );
    let wire = symbrain_usage::report_json(&report).expect("report JSON");
    assert!(wire.starts_with(r#"{"schema_version":1,"providers"#));
}

#[test]
fn provider_cases_bind_requests_responses_auth_and_errors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/provider_cases.json")).expect("provider cases");
    let providers = fixture["providers"].as_array().expect("providers");
    assert_eq!(providers.len(), 10);
    for provider in providers {
        assert!(provider["request"]["method"].is_string());
        assert!(provider["request"]["url"].is_string());
        assert!(!provider["response"].is_null());
        assert!(provider["auth"]["status"].is_string());
        assert_eq!(provider["errors"].as_array().expect("errors").len(), 3);
    }
}

#[test]
fn provider_graph_fixture_is_source_bound_to_go_contract() {
    let fixture: serde_json::Value = serde_json::from_str(GO_ORACLE).expect("Go oracle fixture");
    assert_eq!(fixture["schema_version"], 1);
    let ids: Vec<_> = fixture["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .map(|provider| provider["id"].as_str().expect("provider id"))
        .collect();
    assert_eq!(ids.len(), 10);
    assert_eq!(ids[0], "claude");
    assert_eq!(ids[9], "antigravity");
}

#[test]
fn status_errors_keep_provider_specific_taxonomy_and_retry_detail() {
    let mut responses = BTreeMap::new();
    responses.insert(
        "cursor".into(),
        Response {
            status: 429,
            body: br#"{"error":"rate limited"}"#.to_vec(),
            headers: [("Retry-After".into(), "17".into())].into_iter().collect(),
        },
    );
    let report = Service::with_transport(
        vec![Provider::fixture("cursor", "Cursor")],
        Arc::new(symbrain_usage::FixtureTransport::new(responses)),
    )
    .report();
    let error = report.providers[0].error.as_deref().expect("status error");
    assert!(error.contains("rate limited"));
    assert!(error.contains("retry in 17s"));
}

struct SlowTransport;

impl Transport for SlowTransport {
    fn request(&self, _request: symbrain_usage::Request) -> Result<Response, String> {
        std::thread::sleep(Duration::from_millis(100));
        Ok(Response {
            status: 200,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        })
    }

    fn request_with_cancel(
        &self,
        _request: symbrain_usage::Request,
        cancel: &symbrain_usage::Cancellation,
    ) -> Result<Response, String> {
        let deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err("request cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(Response {
            status: 200,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        })
    }
}

#[test]
fn provider_queries_are_bounded_and_timeout_all_pending_slots() {
    let service = Service::with_transport(
        vec![
            Provider::fixture("one", "One"),
            Provider::fixture("two", "Two"),
        ],
        Arc::new(SlowTransport),
    )
    .with_provider_timeout(Duration::from_millis(10));
    let started = Instant::now();
    let report = service.report();
    assert!(started.elapsed() < Duration::from_millis(80));
    assert!(report.providers.iter().all(|provider| {
        provider
            .error
            .as_deref()
            .is_some_and(|error| error.contains("timed out"))
    }));
}

fn canonical_request(
    request: &symbrain_usage::Request,
    expected: &serde_json::Value,
) -> serde_json::Value {
    let allowed_headers = [
        "Accept",
        "Content-Type",
        "Connect-Protocol-Version",
        "X-Title",
        "X-Server-Id",
        "anthropic-beta",
    ];
    let headers = allowed_headers
        .iter()
        .filter_map(|name| {
            request.headers.get(*name).map(|value| {
                (
                    (*name).to_string(),
                    serde_json::Value::String(value.clone()),
                )
            })
        })
        .collect::<serde_json::Map<_, _>>();
    let mut actual = serde_json::Map::new();
    actual.insert("method".into(), request.method.clone().into());
    actual.insert(
        "url".into(),
        request
            .url
            .replace("127.0.0.1:0", "127.0.0.1:<port>")
            .replace("127.0.0.1:43123", "127.0.0.1:<port>")
            .into(),
    );
    actual.insert("headers".into(), headers.into());
    if expected.get("body").is_some() {
        actual.insert(
            "body".into(),
            String::from_utf8(request.body.clone().expect("request body"))
                .unwrap()
                .into(),
        );
    }
    actual.into()
}

fn canonical_snapshot(
    mut actual: serde_json::Value,
    expected: &serde_json::Value,
) -> serde_json::Value {
    actual["fetched_at"] = expected["fetched_at"].clone();
    let expected_meters = expected["meters"].as_array().expect("expected meters");
    let meters = actual["meters"].as_array_mut().expect("actual meters");
    for meter in meters {
        let label = meter["label"].as_str().expect("meter label");
        let expected_meter = expected_meters
            .iter()
            .find(|candidate| candidate["label"] == label)
            .unwrap_or_else(|| panic!("unexpected meter {label}"));
        if expected_meter.get("resets_at").is_some() {
            meter["resets_at"] = expected_meter["resets_at"].clone();
        } else if let Some(object) = meter.as_object_mut() {
            object.remove("resets_at");
        }
    }
    actual
}

fn expected_strategy(provider: &serde_json::Value) -> (&str, Vec<&str>) {
    (
        provider["strategy"].as_str().expect("strategy"),
        provider["fallback"]
            .as_array()
            .expect("fallback")
            .iter()
            .map(|source| source.as_str().expect("fallback source"))
            .collect(),
    )
}

#[test]
fn provider_cases_compare_complete_requests_headers_strategies_snapshots_and_errors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/provider_cases.json")).expect("provider cases");
    for case in fixture["providers"].as_array().expect("providers") {
        let id = case["id"].as_str().expect("provider id");
        let body = serde_json::to_vec(&case["response"]).expect("response body");
        let transport = symbrain_usage::FixtureTransport::new(
            [(
                id.to_string(),
                Response {
                    status: 200,
                    body,
                    headers: BTreeMap::new(),
                },
            )]
            .into_iter()
            .collect(),
        );
        let report = Service::with_transport(
            vec![symbrain_usage::Provider::fixture(
                id,
                case["id"].as_str().unwrap(),
            )],
            Arc::new(transport.clone()),
        )
        .report();
        let provider = &report.providers[0];
        assert_eq!(
            provider.configured,
            case["auth"]["configured"].as_bool().unwrap(),
            "{id} auth configured"
        );
        assert_eq!(
            provider.auth_status.status,
            case["auth"]["status"].as_str().unwrap(),
            "{id} auth status"
        );
        assert_eq!(
            provider.auth_status.source.as_deref(),
            case["auth"]["source"].as_str(),
            "{id} auth source"
        );
        let snapshot = provider
            .snapshot
            .as_ref()
            .unwrap_or_else(|| panic!("{id} snapshot"));
        assert_eq!(
            snapshot.source,
            case["strategy"].as_str().unwrap(),
            "{id} strategy"
        );
        assert_eq!(
            vec![snapshot.source.as_str()],
            expected_strategy(case).1,
            "{id} fallback"
        );
        assert_eq!(
            canonical_request(
                transport.requests().last().expect("request"),
                &case["request"]
            ),
            case["request"],
            "{id} request",
        );
        let actual_snapshot =
            canonical_snapshot(serde_json::to_value(snapshot).unwrap(), &case["snapshot"]);
        let actual_snapshot_bytes = serde_json::to_vec(&actual_snapshot).unwrap();
        let expected_snapshot_bytes = serde_json::to_vec(&case["snapshot"]).unwrap();
        assert_eq!(
            actual_snapshot_bytes, expected_snapshot_bytes,
            "{id} snapshot bytes",
        );
        assert_fixture_errors(id, case);
    }
}

fn assert_fixture_errors(id: &str, case: &serde_json::Value) {
    for error_case in case["errors"].as_array().expect("errors") {
        let status = u16::try_from(error_case["status"].as_u64().unwrap_or(200))
            .expect("fixture status fits in u16");
        let mut headers = BTreeMap::new();
        if status == 429 {
            headers.insert("Retry-After".into(), "17".into());
        }
        let body = if status == 200 {
            b"{}".to_vec()
        } else {
            serde_json::to_vec(&case["response"]).unwrap()
        };
        let transport = symbrain_usage::FixtureTransport::new(
            [(
                id.to_string(),
                Response {
                    status,
                    body,
                    headers,
                },
            )]
            .into_iter()
            .collect(),
        );
        let report = Service::with_transport(
            vec![symbrain_usage::Provider::fixture(id, id)],
            Arc::new(transport),
        )
        .report();
        assert_eq!(
            report.providers[0].error.as_deref(),
            error_case["text"].as_str(),
            "{id} {:?} error",
            error_case["kind"],
        );
    }
}
