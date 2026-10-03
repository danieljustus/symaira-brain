//! Fresh immutable Go Hermes constructor/request replay without live endpoints.
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use symbrain_usage::{Request, Response, Service, Transport};

struct Unauthorized(Mutex<Vec<String>>);
impl Transport for Unauthorized {
    fn request(&self, request: Request) -> Result<Response, String> {
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        self.0.lock().expect("requests").push(authorization);
        Ok(Response {
            status: 401,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        })
    }
}

#[allow(unsafe_code)]
fn env(name: &str, value: &str) {
    unsafe { std::env::set_var(name, value) };
}

fn prepare(record: &serde_json::Value) {
    // This dedicated integration binary has one test. Each scoped report joins
    // all workers before the next environment update.
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
        "CODEX_HOME",
        "KIMI_CODE_HOME",
        "HERMES_PORTAL_BASE_URL",
        "KIMI_CODE_BASE_URL",
        "OPENROUTER_API_URL",
        "OPENCODE_WORKSPACE_ID",
        "USAGE_HERMES_ABSENT",
    ] {
        env(name, "");
    }
    env("ANTHROPIC_OAUTH_TOKEN", "env://USAGE_HERMES_ABSENT");
    env("HOME", record["home"].as_str().expect("home"));
    env("USERPROFILE", record["home"].as_str().expect("home"));
    env(
        "HERMES_HOME",
        record["hermes_home"].as_str().expect("Hermes root"),
    );
    env("PATH", "");
}

#[test]
#[ignore = "fresh immutable Go evidence supplied by the native Hermes gate"]
fn usage_hermes_768_matches_fresh_go_file_and_jwt_contracts() {
    let fixture = std::env::var("USAGE_HERMES_ORACLE").expect("fresh Go Hermes fixture required");
    let input = std::env::var("USAGE_HERMES_INPUT").expect("source-bound input required");
    let input: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).expect("source cases")).expect("cases");
    let records: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture).expect("Go Hermes evidence"))
            .expect("oracle");
    assert_eq!(records.len(), 92, "complete Hermes corpus");
    let expected: BTreeSet<_> = input
        .iter()
        .map(|record| record["id"].as_str().expect("input id"))
        .collect();
    let observed: BTreeSet<_> = records
        .iter()
        .map(|record| record["id"].as_str().expect("record id"))
        .collect();
    assert_eq!(expected.len(), 92, "source cases must be distinct");
    assert_eq!(observed, expected, "exact Hermes case coverage");
    for record in &records {
        prepare(record);
        assert!(
            !symbrain_usage::needs_go_fallback(),
            "{}: native Hermes routing",
            record["id"]
        );
        let path = record["auth_path"].as_str().expect("store path");
        let before = std::fs::read(path).ok();
        assert_eq!(
            before.is_some(),
            record["file_present"].as_bool().expect("file presence")
        );
        if let Some(contents) = &before {
            assert_eq!(
                format!("{:x}", Sha256::digest(contents)),
                record["sha256"].as_str().expect("source hash")
            );
        }
        let providers = symbrain_usage::all_providers()
            .into_iter()
            .filter(|provider| provider.id == "nous")
            .collect();
        let transport = Arc::new(Unauthorized(Mutex::new(Vec::new())));
        let report = Service::with_transport(providers, transport.clone()).report();
        assert_eq!(
            serde_json::to_value(report).expect("report"),
            record["report"],
            "{}: complete Hermes report",
            record["id"]
        );
        assert_eq!(
            serde_json::to_value(transport.0.lock().expect("requests").clone()).expect("headers"),
            record["authorization"],
            "{}: literal file token",
            record["id"]
        );
        assert_eq!(
            std::fs::read(path).ok(),
            before,
            "{}: no store mutation",
            record["id"]
        );
    }
    if let Ok(path) = std::env::var("USAGE_HERMES_NATIVE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({"cases":92,"passed":92,"failed":0}))
                .expect("receipt"),
        )
        .expect("write receipt");
    }
}
