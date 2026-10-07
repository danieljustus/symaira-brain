//! Fresh file-reader/constructor replay; automatic system Keychain is isolated.
use crate::{Request, Response, Service, Transport};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

struct Unauthorized(Mutex<Vec<String>>);
impl Transport for Unauthorized {
    fn request(&self, request: Request) -> Result<Response, String> {
        let header = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        self.0.lock().expect("requests").push(header);
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
        "HERMES_HOME",
        "KIMI_CODE_HOME",
        "HERMES_PORTAL_BASE_URL",
        "KIMI_CODE_BASE_URL",
        "OPENROUTER_API_URL",
        "OPENCODE_WORKSPACE_ID",
        "USAGE_FILES_ABSENT",
    ] {
        env(name, "");
    }
    let home = record["home"].as_str().expect("private home");
    env("HOME", home);
    env("USERPROFILE", home);
    env("PATH", "");
    env(
        "CODEX_HOME",
        record["codex_home_override"].as_str().expect("override"),
    );
}
#[test]
#[ignore = "fresh immutable Go file evidence supplied by provider-files gate"]
fn provider_files_oracle_matches_fresh_go() {
    let fixture = std::env::var("USAGE_FILES_ORACLE").expect("fresh Go fixture required");
    let input = std::env::var("USAGE_FILES_INPUT").expect("source-bound cases required");
    let input: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).expect("source cases")).expect("cases");
    let records: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture).expect("Go provider files evidence"))
            .expect("oracle");
    assert_eq!(records.len(), 86, "complete provider file corpus");
    let expected: BTreeSet<_> = input
        .iter()
        .map(|r| r["id"].as_str().expect("input id"))
        .collect();
    let actual: BTreeSet<_> = records
        .iter()
        .map(|r| r["id"].as_str().expect("record id"))
        .collect();
    assert_eq!(expected.len(), 86, "distinct source cases");
    assert_eq!(actual, expected, "exact provider file case coverage");
    let mut compared = 0;
    let mut gated = 0;
    for record in &records {
        if compare_record(record) {
            compared += 1;
        } else {
            gated += 1;
        }
    }
    assert_eq!((compared, gated), (85, 1));
    if let Ok(path) = std::env::var("USAGE_FILES_NATIVE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"cases":86,"passed":86,"failed":0,"full_reports":85,"gated":1}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}

fn compare_record(record: &serde_json::Value) -> bool {
    prepare(record);
    // Eligibility is checked without inspecting the operator's Keychain.
    // Constructor proof separately uses the same private injection point
    // as frozen Go; it proves ordering, not host ACL access.
    env("ANTHROPIC_OAUTH_TOKEN", "env://USAGE_FILES_ABSENT");
    let ambiguous = record["ambiguous"].as_bool().expect("ambiguity marker");
    assert_eq!(
        super::needs_go_fallback(),
        ambiguous,
        "{}: native eligibility",
        record["id"]
    );
    let path = record["auth_path"].as_str().expect("path");
    let before = std::fs::read(path).ok();
    assert_eq!(before.is_some(), record["file_present"].as_bool().unwrap());
    if let Some(contents) = &before {
        assert_eq!(
            format!("{:x}", Sha256::digest(contents)),
            record["sha256"].as_str().unwrap()
        );
    }
    if ambiguous {
        assert!(super::claude_file_token_in(std::path::Path::new(path)).is_none());
        assert!(
            record["report"]["providers"][0]["configured"]
                .as_bool()
                .unwrap()
        );
        assert!(
            ["Bearer spare-token", "Bearer work-token"]
                .contains(&record["authorization"][0].as_str().unwrap())
        );
        assert_eq!(std::fs::read(path).ok(), before, "ambiguous file unchanged");
        return false;
    }
    let calls = Cell::new(0);
    let provider = if record["provider"] == "claude" {
        env(
            "ANTHROPIC_OAUTH_TOKEN",
            record["env_token"].as_str().unwrap(),
        );
        super::claude_with_keychain(|| {
            calls.set(calls.get() + 1);
            None
        })
    } else {
        env("CODEX_ACCESS_TOKEN", record["env_token"].as_str().unwrap());
        super::codex()
    };
    let transport = Arc::new(Unauthorized(Mutex::new(Vec::new())));
    let report = Service::with_transport(vec![provider], transport.clone()).report();
    assert_eq!(
        serde_json::to_value(report).expect("report"),
        record["report"],
        "{}: complete provider file report",
        record["id"]
    );
    assert_eq!(
        serde_json::to_value(transport.0.lock().unwrap().clone()).unwrap(),
        record["authorization"],
        "{}: literal file request token",
        record["id"]
    );
    assert_eq!(
        calls.get(),
        record["keychain_calls"].as_u64().unwrap(),
        "{}: last-source ordering",
        record["id"]
    );
    assert_eq!(
        std::fs::read(path).ok(),
        before,
        "{}: no store mutation",
        record["id"]
    );
    true
}
