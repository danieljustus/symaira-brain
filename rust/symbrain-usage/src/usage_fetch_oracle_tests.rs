//! Fresh source-bound reports and requests for #620; no live credentials/network.
use super::provider_config::{
    claude_from_resolved, codex_from_resolved, copilot_from_resolved, cursor_from_resolved,
    kimi_from_resolved, moonshot_from_resolved, nous_from_resolved, opencode_from_resolved,
    openrouter_from_resolved,
};
use super::provider_identity::{KIMI_COMPATIBILITY_VERSION, hostname, platform_label};
use crate::{FixtureTransport, Provider, Report, Response, Service};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

const CREDENTIAL: &str = "oracle-only-invalid-620-credential";
const DEVICE: &str = "oracle-620-device";
const PROCESS_LIST: &str = "123 /Applications/Antigravity/language_server --app_data_dir antigravity --csrf_token fixture\n";

fn execute(provider: Provider, response: Response) -> (Report, FixtureTransport) {
    if provider.id != "antigravity" {
        let transport = FixtureTransport::new([(provider.id.clone(), response)].into());
        let report = Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
        return (report, transport);
    }
    let replies = vec![
        Ok(Response {
            status: 200,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        }),
        Ok(response.clone()),
        Ok(response.clone()),
        Ok(response),
    ];
    let transport = FixtureTransport::with_sequences([("antigravity".into(), replies)].into());
    let transport_ref: Arc<dyn crate::Transport> = Arc::new(transport.clone());
    let result = super::wrap_fetch_error(
        &provider.id,
        super::fetch_antigravity_from_observations(
            &provider,
            &transport_ref,
            &crate::Cancellation::new(),
            PROCESS_LIST,
            |_| Some("COMMAND PID NAME\nlanguage 123 TCP 127.0.0.1:43123 (LISTEN)\n".into()),
        ),
    );
    let mut row = crate::ProviderUsage::from_provider(&provider);
    match result {
        Ok(snapshot) => row.snapshot = Some(snapshot),
        Err(error) => row.error = Some(error.to_string()),
    }
    (
        Report {
            schema_version: 1,
            providers: vec![row],
        },
        transport,
    )
}

fn provider(variant: &str) -> Provider {
    let credential = Some(("env".into(), CREDENTIAL.into()));
    match variant {
        "claude-api" => claude_from_resolved(credential, None, None, None, None),
        "claude-oauth" => claude_from_resolved(None, None, credential, None, None),
        "claude-chain" => claude_from_resolved(credential.clone(), None, credential, None, None),
        "codex" => codex_from_resolved(credential, None, false),
        "copilot" => copilot_from_resolved(credential, None),
        "copilot-enterprise" => copilot_from_resolved(credential, None)
            .with_copilot_enterprise_host("copilot.example.com"),
        "copilot-enterprise-port" => copilot_from_resolved(credential, None)
            .with_copilot_enterprise_host("copilot.example.com:8443"),
        "cursor" => cursor_from_resolved(credential, None),
        "antigravity" => super::provider_config::antigravity_from_process_list(Some(PROCESS_LIST)),
        variant if variant.starts_with("kimi-") => kimi_from_resolved(
            matches!(variant, "kimi-api" | "kimi-chain")
                .then(|| credential.clone())
                .flatten(),
            None,
            matches!(variant, "kimi-cli" | "kimi-cli-no-device" | "kimi-chain")
                .then_some(CREDENTIAL),
            matches!(variant, "kimi-web" | "kimi-chain")
                .then_some(credential)
                .flatten(),
            None,
            "https://api.kimi.com".into(),
            (variant != "kimi-cli-no-device").then(|| DEVICE.into()),
        ),
        "moonshot-ai" => moonshot_from_resolved(credential, None, "ai"),
        "moonshot-cn" => moonshot_from_resolved(credential, None, "cn"),
        "nous" => nous_from_resolved(credential, None, "https://portal.nousresearch.com".into()),
        "opencode" => opencode_from_resolved(credential, None, Some("wrk_fixture")),
        "openrouter" => {
            openrouter_from_resolved(credential, None, "https://openrouter.ai/api/v1".into())
        }
        _ => panic!("unknown fixture variant: {variant}"),
    }
}

#[test]
fn usage_fetch_620_reports_and_all_identity_headers_match_go() {
    let fresh_path = std::env::var_os("USAGE_FETCH_ORACLE_620");
    let data = fresh_path.as_ref().map_or_else(
        || include_bytes!("../tests/fixtures/usage_fetch_620.json").to_vec(),
        |path| std::fs::read(path).expect("fresh Go oracle"),
    );
    let cases: Vec<Value> = serde_json::from_slice(&data).expect("Go matrix");
    assert_eq!(
        cases.len(),
        133,
        "all 19 variants and seven responses required"
    );
    let mut actual_reports = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id.to_owned()), "duplicate case: {id}");
        let variant = case["variant"].as_str().expect("variant");
        let provider = provider(variant);
        let response = Response {
            status: u16::try_from(case["reply"]["status"].as_u64().expect("status"))
                .expect("HTTP status"),
            body: case["reply"]["body"]
                .as_str()
                .expect("body")
                .as_bytes()
                .to_vec(),
            headers: serde_json::from_value(case["reply"]["headers"].clone())
                .expect("response headers"),
        };
        let (mut report, transport) = execute(provider, response);
        if let Some(snapshot) = &mut report.providers[0].snapshot {
            snapshot.fetched_at = chrono::DateTime::UNIX_EPOCH;
        }
        assert_eq!(
            serde_json::to_value(&report).expect("native report"),
            case["report"],
            "{id}: full report"
        );
        let requests = transport.requests();
        let expected = case["requests"].as_array().expect("Go requests");
        assert_eq!(requests.len(), expected.len(), "{id}: full request walk");
        let mut actual_requests = Vec::new();
        for (request, expected) in requests.iter().zip(expected) {
            let mut headers: BTreeMap<String, String> = request
                .headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
                .collect();
            if let Some(instance) = headers.get_mut("x-server-instance") {
                assert!(instance.starts_with("server-fn:"));
                *instance = "server-fn:<random>".into();
            }
            let mut expected = expected.clone();
            if fresh_path.is_none() && expected["headers"].get("x-msh-platform").is_some() {
                // Only the historic local recording requires OS substitutions;
                // the fresh native CI oracle compares actual OS facts verbatim.
                expected["headers"]["x-msh-platform"] = platform_label().into();
                expected["headers"]["x-msh-device-name"] = hostname().into();
                let display = if cfg!(target_os = "macos") {
                    "macOS"
                } else {
                    platform_label()
                };
                expected["headers"]["x-msh-device-model"] =
                    format!("{display} {KIMI_COMPATIBILITY_VERSION}").into();
            }
            let actual = json!({"method":request.method,"url":request.url,"headers":headers,
                "body":String::from_utf8(request.body.clone().unwrap_or_default()).expect("request UTF-8")});
            assert_eq!(actual, expected, "{id}: exact request and all headers");
            actual_requests.push(actual);
        }
        actual_reports.push(json!({"id":id,"report":report,"requests":actual_requests}));
    }
    if let Some(path) = std::env::var_os("USAGE_FETCH_NATIVE_620") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&actual_reports).expect("native reports"),
        )
        .expect("save native results for the real CLI renderer");
    }
}

#[test]
fn copilot_enterprise_authority_keeps_transport_validation() {
    for host in [
        "127.0.0.1",
        "10.0.0.1",
        "user:password@copilot.example.com",
        "copilot.example.com:0",
        "copilot.example.com:bad",
    ] {
        let provider = provider("copilot").with_copilot_enterprise_host(host);
        let transport: Arc<dyn crate::Transport> = Arc::new(crate::UreqTransport::default());
        let error = provider
            .fetch(&transport, &crate::Cancellation::new())
            .expect_err("unsafe authority");
        assert!(
            error.to_string().contains("provider URL must use HTTPS"),
            "{host}: {error}"
        );
    }
    for host in ["", "github.com"] {
        assert_eq!(
            super::provider_identity::copilot_url(Some(host)),
            "https://api.github.com/copilot_internal/user"
        );
    }
}
