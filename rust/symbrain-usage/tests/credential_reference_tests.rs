//! Fresh Go constructor/credential/request replay with synthetic child providers.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use symbrain_usage::{Request, Response, Service, Transport};

struct UnauthorizedTransport(Mutex<Vec<BTreeMap<String, String>>>);
impl Transport for UnauthorizedTransport {
    fn request(&self, request: Request) -> Result<Response, String> {
        let headers = request
            .headers
            .into_iter()
            .filter_map(|(key, value)| {
                let key = key.to_ascii_lowercase();
                ["authorization", "x-api-key", "cookie"]
                    .contains(&key.as_str())
                    .then_some((key, value))
            })
            .collect();
        self.0.lock().expect("request capture").push(headers);
        Ok(Response {
            status: 401,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        })
    }
}

// This integration test binary contains one test. All provider workers finish
// before the next environment change; subprocesses see only owned test roots.
#[allow(unsafe_code)]
fn set(name: &str, value: &str) {
    unsafe { std::env::set_var(name, value) };
}

#[test]
#[ignore = "fresh immutable Go evidence supplied by the native credential gate"]
fn usage_credential_768_matches_fresh_go_constructors_and_requests() {
    let path = std::env::var("USAGE_768_ORACLE").expect("fresh Go credential evidence required");
    let records: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).expect("Go evidence")).expect("oracle JSON");
    assert_eq!(
        records.len(),
        44,
        "nonzero complete 11-provider-source × 4-reference corpus"
    );
    validate_corpus(&records);
    for record in &records {
        for name in [
            "ANTHROPIC_ADMIN_KEY",
            "ANTHROPIC_OAUTH_TOKEN",
            "CODEX_ACCESS_TOKEN",
            "COPILOT_ACCESS_TOKEN",
            "CURSOR_COOKIE",
            "KIMI_CODE_API_KEY",
            "KIMI_AUTH_TOKEN",
            "MOONSHOT_API_KEY",
            "NOUS_PORTAL_ACCESS_TOKEN",
            "OPENCODE_COOKIE",
            "OPENROUTER_API_KEY",
            "USAGE_768_ABSENT",
            "CODEX_HOME",
            "HERMES_HOME",
            "KIMI_CODE_HOME",
            "KIMI_CODE_BASE_URL",
            "HERMES_PORTAL_BASE_URL",
            "OPENROUTER_API_URL",
            "SECRET_ORACLE_FAKE_STDERR",
            "SECRET_ORACLE_FAKE_SLEEP_MS",
            "SECRET_ORACLE_FAKE_STDOUT_BYTES",
        ] {
            set(name, "");
        }
        for (name, value) in record["env"].as_object().expect("case env") {
            set(name, value.as_str().expect("env string"));
        }
        assert!(
            !symbrain_usage::needs_go_fallback(),
            "{}: native routing",
            record["id"]
        );
        let providers = symbrain_usage::all_providers()
            .into_iter()
            .filter(|provider| provider.id == record["provider"].as_str().expect("provider"))
            .collect();
        let transport = Arc::new(UnauthorizedTransport(Mutex::new(Vec::new())));
        let report = Service::with_transport(providers, transport.clone()).report();
        assert_eq!(
            serde_json::to_value(report).expect("report"),
            record["report"],
            "{}: full report",
            record["id"]
        );
        assert_eq!(
            serde_json::to_value(transport.0.lock().expect("headers").clone()).expect("headers"),
            record["headers"],
            "{}: resolved request credentials",
            record["id"]
        );
    }
    if let Ok(path) = std::env::var("USAGE_768_NATIVE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"cases":records.len(),"passed":records.len(),"failed":0}),
            )
            .expect("receipt"),
        )
        .expect("write receipt");
    }
}

fn validate_corpus(records: &[serde_json::Value]) {
    let expected: BTreeSet<String> = [
        "ANTHROPIC_ADMIN_KEY",
        "ANTHROPIC_OAUTH_TOKEN",
        "CODEX_ACCESS_TOKEN",
        "COPILOT_ACCESS_TOKEN",
        "CURSOR_COOKIE",
        "KIMI_CODE_API_KEY",
        "KIMI_AUTH_TOKEN",
        "MOONSHOT_API_KEY",
        "NOUS_PORTAL_ACCESS_TOKEN",
        "OPENCODE_COOKIE",
        "OPENROUTER_API_KEY",
    ]
    .into_iter()
    .flat_map(|name| {
        [
            "env://USAGE_768_TOKEN",
            "symvault://test/token",
            "vault://test/token",
            "keychain://test/account",
        ]
        .into_iter()
        .map(move |reference| format!("{name}-{reference}"))
    })
    .collect();
    let observed: BTreeSet<String> = records
        .iter()
        .map(|record| record["id"].as_str().expect("case id").to_owned())
        .collect();
    assert_eq!(observed, expected, "exact source/reference corpus");
}
