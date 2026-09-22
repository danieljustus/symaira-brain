//! Replays the `OpenCode` discovery corpus recorded by
//! `scripts/usage-request-oracle/opencode` — the shipped Go provider's
//! executed sequences over a deterministic recording transport (canned
//! responses, no network, no live provider data). The port must reproduce
//! every executed request, the workspace-id normalization, the parsed
//! snapshot, and the error text, case for case.

use super::Provider;
use super::provider_requests::normalize_workspace;
use crate::model::UsageSnapshot;
use crate::transport::{Cancellation, FixtureTransport, Response, Transport};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const ORACLE: &str = include_str!("../tests/fixtures/opencode_discovery.json");

/// The corpus case ids in fixture order. The replay asserts this list so a
/// fixture edit that adds, drops, or renames a case fails until the list is
/// updated deliberately.
const CASE_IDS: [&str; 33] = [
    "workspace_then_subscription_ok",
    "workspace_post_fallback_ok",
    "workspace_lookup_missing_id",
    "workspace_signed_out_get",
    "workspace_signed_out_post",
    "workspace_http_401",
    "workspace_http_500_signed_out_body",
    "workspace_http_500",
    "subscription_override_ok",
    "subscription_url_override_ok",
    "subscription_post_fallback_ok",
    "subscription_unparseable_both",
    "subscription_signed_out_get",
    "subscription_signed_out_post",
    "network_error_first_request",
    "workspace_only_no_cookie",
    "normalize_bare_id",
    "normalize_url_full",
    "normalize_url_padded",
    "normalize_embedded_text",
    "normalize_empty",
    "normalize_url_no_id",
    "normalize_wrk_underscore_only",
    "normalize_token_prefix",
    "normalize_url_query",
    "normalize_url_fragment",
    "normalize_bad_percent",
    "normalize_dump_workspace",
    "normalize_wrk_space",
    "normalize_wrk_quote",
    "normalize_wrk_html",
    "normalize_wrk_star",
    "normalize_wrk_line_sep",
];

/// The Go chain runner's wrapper around strategy failures. It is only peeled
/// off to recover the injected transport detail of the network-error case
/// (see `scripted_transport`).
const CHAIN_PREFIX: &str = "all AI usage fallbacks failed: ";

/// The shipped network wrapper (`openCodeError` kind=network).
const NETWORK_PREFIX: &str = "OpenCode request failed: ";

#[test]
fn corpus_replays_case_for_case_against_the_shipped_recording() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("oracle fixture must parse");
    let cases = oracle["cases"].as_array().expect("cases array");
    let ids: Vec<&str> = cases
        .iter()
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    assert_eq!(ids, CASE_IDS, "declared case list drifted from the fixture");
    for case in cases {
        replay(case);
    }
}

#[test]
fn provenance_binds_the_corpus_to_the_shipped_opencode_source() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("oracle fixture must parse");
    let provenance = oracle.get("provenance").expect("provenance object");
    assert_eq!(
        provenance["generator"],
        "scripts/usage-request-oracle/opencode/main.go"
    );
    assert_eq!(
        provenance["command"],
        "go run ./scripts/usage-request-oracle/opencode"
    );
    assert_eq!(
        provenance["check_command"],
        "go run ./scripts/usage-request-oracle/opencode -check"
    );
    assert_eq!(provenance["oracle_source"], "internal/usage/opencode.go");
    let sources = provenance["oracle_sources"]
        .as_array()
        .expect("oracle_sources array");
    assert_eq!(
        sources.len(),
        2,
        "the corpus spans opencode.go and opencode_parse.go"
    );
    let transport = provenance["transport"].as_str().expect("transport label");
    assert!(
        transport.contains("no network"),
        "transport must state that it is not live provider evidence"
    );
    // The corpus is only as trustworthy as its anchors: each one must exist
    // verbatim in one of the shipped Go sources this slice ports.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut shipped = String::new();
    for source in sources {
        let source_path = manifest
            .join("../..")
            .join(source.as_str().expect("source path"));
        shipped.push_str(
            &std::fs::read_to_string(&source_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", source_path.display())),
        );
    }
    let anchors = provenance["oracle_anchors"]
        .as_array()
        .expect("oracle_anchors");
    assert!(!anchors.is_empty(), "provenance must carry anchors");
    for anchor in anchors {
        let anchor = anchor.as_str().expect("anchor string");
        assert!(
            shipped.contains(anchor),
            "anchor missing from the shipped OpenCode sources: {anchor}"
        );
    }
}

/// Rebuilds the recorder's canned answers from the recorded script: status
/// steps become responses; the network step re-injects the exact transport
/// detail recovered from the recorded error text (chain and network wrappers
/// peeled off), so even the failure text compares exactly.
fn scripted_transport(case: &Value) -> Vec<Result<Response, String>> {
    // Cases that never issue a request record a null script.
    let Some(script) = case["script"].as_array() else {
        return Vec::new();
    };
    script
        .iter()
        .map(|step| {
            if let Some(raw) = step.get("error").and_then(Value::as_str) {
                let recorded = case
                    .pointer("/result/text")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        panic!("{}: network step records its error text", case["id"])
                    });
                let injected = recorded
                    .strip_prefix(CHAIN_PREFIX)
                    .and_then(|inner| inner.strip_prefix(NETWORK_PREFIX))
                    .unwrap_or(raw);
                return Err(injected.to_string());
            }
            let status = u16::try_from(step.get("status").and_then(Value::as_u64).unwrap_or(200))
                .expect("recorded status fits u16");
            let body = step
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .as_bytes()
                .to_vec();
            Ok(Response {
                status,
                body,
                headers: BTreeMap::new(),
            })
        })
        .collect()
}

/// One recorded `OPENCODE_*` environment value, empty when unset (Go's
/// environment resolution filters empties the same way).
fn env_value<'a>(case: &'a Value, key: &str) -> &'a str {
    case.get("env")
        .and_then(Value::as_object)
        .and_then(|env| env.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn replay(case: &Value) {
    let id = case["id"].as_str().expect("case id");

    let mut provider = Provider::fixture("opencode", "opencode");
    provider.fixture = false;
    provider.credentials.clear();
    provider.credential = None;
    let cookie = env_value(case, "OPENCODE_COOKIE");
    if !cookie.is_empty() {
        provider
            .credentials
            .push(("env".into(), cookie.to_string()));
        provider.credential = Some(cookie.to_string());
    }
    let workspace_input = env_value(case, "OPENCODE_WORKSPACE_ID");
    if !workspace_input.is_empty() {
        provider
            .credentials
            .push(("workspace".into(), workspace_input.to_string()));
    }

    // The workspace the recording observed must equal what normalization
    // derives from the recorded environment. Cases whose workspace lookup
    // never issued a request omit the field.
    if let Some(observed) = case.get("workspace").and_then(Value::as_str) {
        assert_eq!(
            normalize_workspace(workspace_input),
            observed,
            "{id}: workspace normalization"
        );
    }

    let fixture = FixtureTransport::with_sequences(
        [("opencode".to_string(), scripted_transport(case))]
            .into_iter()
            .collect(),
    );
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let cancel = Cancellation::with_timeout(Duration::from_secs(5));
    let result = provider.fetch(&transport, &cancel);

    // Exact executed-case equality against the recording.
    let recorded = case["executed"].as_array().expect("executed array");
    let executed = fixture.requests();
    assert_eq!(
        executed.len(),
        recorded.len(),
        "{id}: executed request count"
    );
    for (index, request) in executed.iter().enumerate() {
        let expected = &recorded[index];
        assert_eq!(
            request.method,
            expected["method"].as_str().unwrap_or_default(),
            "{id}[{index}]: method"
        );
        assert_eq!(
            request.url,
            expected["url"].as_str().unwrap_or_default(),
            "{id}[{index}]: url"
        );
        let body = request.body.as_ref().map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
        assert_eq!(
            body,
            expected["body"].as_str().unwrap_or_default(),
            "{id}[{index}]: body"
        );

        let mut headers = normalize_headers(request, cookie);
        let mut expected_headers: BTreeMap<String, String> = BTreeMap::new();
        for header in expected["headers"].as_array().expect("headers array") {
            expected_headers.insert(
                header["name"]
                    .as_str()
                    .expect("header name")
                    .to_ascii_lowercase(),
                header["value"].as_str().expect("header value").to_string(),
            );
        }
        // X-Server-Instance carries a per-request random id; only its shape
        // is reproducible (the recording carries the same placeholder rule).
        let expected_instance = expected_headers.remove("x-server-instance");
        let instance = headers.remove("x-server-instance");
        assert_eq!(
            instance.map(|value| value.starts_with("server-fn:")),
            expected_instance.map(|value| value.starts_with("server-fn:")),
            "{id}[{index}]: X-Server-Instance shape"
        );
        assert_eq!(headers, expected_headers, "{id}[{index}]: headers");
    }

    // Result parity: snapshot fields recorded by the Go oracle, or the exact
    // recorded error text.
    let recorded_result = &case["result"];
    match recorded_result["kind"].as_str().expect("result kind") {
        "ok" => {
            let snapshot = result.unwrap_or_else(|error| {
                panic!("{id}: expected a snapshot from the recording, got {error}")
            });
            assert_eq!(snapshot.source, "web", "{id}: source");
            compare_snapshot(id, &snapshot, recorded_result);
        }
        "error" => {
            let error = result.expect_err("recorded result is an error").to_string();
            let expected = recorded_result["text"]
                .as_str()
                .expect("recorded error text");
            assert_eq!(error, expected, "{id}: error text");
        }
        kind => panic!("{id}: unknown recorded result kind {kind}"),
    }
}

fn compare_snapshot(id: &str, snapshot: &UsageSnapshot, recorded: &Value) {
    let meters = recorded["snapshot"]["meters"]
        .as_array()
        .expect("meters array");
    assert_eq!(snapshot.meters.len(), meters.len(), "{id}: meter count");
    for (index, meter) in snapshot.meters.iter().enumerate() {
        let expected = &meters[index];
        assert_eq!(
            meter.label,
            expected["label"].as_str().expect("meter label"),
            "{id}: meter {index} label"
        );
        assert_eq!(
            meter.used.as_deref(),
            expected["used"].as_str(),
            "{id}: meter {index} used"
        );
        assert_eq!(
            meter.limit.as_deref(),
            expected["limit"].as_str(),
            "{id}: meter {index} limit"
        );
        assert_eq!(
            meter.unit,
            expected["unit"].as_str().unwrap_or_default(),
            "{id}: meter {index} unit"
        );
        assert_eq!(
            meter.resets_at.is_some(),
            expected["resets_at"].as_bool().unwrap_or(false),
            "{id}: meter {index} resets_at"
        );
    }
}

/// Lowercases header names (Go canonicalizes them, the port keeps the source
/// spelling) and maps run-specific values onto the placeholders the recording
/// carries.
fn normalize_headers(
    request: &crate::transport::Request,
    cookie: &str,
) -> BTreeMap<String, String> {
    request
        .headers
        .iter()
        .map(|(name, value)| {
            let value = if name.eq_ignore_ascii_case("Cookie") && value == cookie {
                "CREDENTIAL".to_string()
            } else {
                value.clone()
            };
            (name.to_ascii_lowercase(), value)
        })
        .collect()
}
