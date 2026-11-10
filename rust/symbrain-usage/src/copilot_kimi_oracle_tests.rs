//! Frozen actual-Go constructors and complete synthetic request comparisons.
//! Evidence: `tests/fixtures/usage_768_copilot_kimi_go.json` (`record_usage_evidence.py copilot-kimi`).
use crate::{Request, Response, Service, Transport};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::provider_file_oracle_tests::frozen;

include!("copilot_kimi_owner_tests.rs");
include!("copilot_kimi_divergence.rs");

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

struct Unauthorized(Mutex<Vec<serde_json::Value>>);
impl Transport for Unauthorized {
    fn request(&self, request: Request) -> Result<Response, String> {
        let headers: BTreeMap<_, _> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_lowercase(), vec![value.clone()]))
            .collect();
        let raw_headers: BTreeMap<_, _> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_lowercase(), vec![hex(value.as_bytes())]))
            .collect();
        self.0.lock().unwrap().push(serde_json::json!({
            "method":request.method, "url":request.url, "headers":headers,
            "header_value_hex":raw_headers, "body_hex":hex(request.body.as_deref().unwrap_or_default())
        }));
        Ok(Response {
            status: 401,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        })
    }
}
#[allow(unsafe_code)]
fn env(name: &str, value: &std::ffi::OsStr) {
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
        "CODEX_HOME",
        "KIMI_CODE_HOME",
        "HERMES_HOME",
        "KIMI_CODE_BASE_URL",
        "HERMES_PORTAL_BASE_URL",
        "OPENROUTER_API_URL",
        "OPENCODE_WORKSPACE_ID",
        "USAGE_LOCAL_FILES_ABSENT",
    ] {
        env(name, std::ffi::OsStr::new(""));
    }
    for name in ["HOME", "USERPROFILE"] {
        let key = if name == "HOME" {
            "home"
        } else {
            "userprofile"
        };
        env(name, std::ffi::OsStr::new(record[key].as_str().unwrap()));
    }
    for (name, value) in record["env"].as_object().unwrap() {
        let value = value
            .as_str()
            .unwrap()
            .replace("$HOME", record["home"].as_str().unwrap());
        env(name, std::ffi::OsStr::new(&value));
    }
    // Eligibility must not list/read operator Keychain items on native macOS.
    env(
        "ANTHROPIC_OAUTH_TOKEN",
        std::ffi::OsStr::new("env://USAGE_LOCAL_FILES_ABSENT"),
    );
}
fn normalized_requests(requests: &serde_json::Value) -> serde_json::Value {
    let mut requests = requests.clone();
    for request in requests.as_array_mut().unwrap() {
        for field in ["headers", "header_value_hex"] {
            request[field] = serde_json::Value::Object(
                request[field]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(name, value)| (name.to_lowercase(), value.clone()))
                    .collect(),
            );
        }
        // The device name is the recording host's name, a host fact like the
        // native gates treat it; the replay host sends its own.
        if request["headers"].get("x-msh-device-name").is_some() {
            let host = super::super::provider_identity::hostname();
            request["headers"]["x-msh-device-name"] = serde_json::json!([host]);
            request["header_value_hex"]["x-msh-device-name"] =
                serde_json::json!([hex(host.as_bytes())]);
        }
    }
    requests
}
#[test]
fn copilot_kimi_oracle_matches_frozen_go() {
    frozen::in_child(module_path!(), "copilot_kimi_oracle_replay");
}

#[test]
#[ignore = "child process of copilot_kimi_oracle_matches_frozen_go (mutates process environment)"]
fn copilot_kimi_oracle_replay() {
    // Recorded on darwin; the credential tree, owners and paths are platform specific.
    if !cfg!(target_os = "macos") {
        eprintln!("SKIP frozen Copilot/Kimi evidence was recorded on darwin; running elsewhere");
        return;
    }
    let evidence = frozen::restore(include_str!(
        "../tests/fixtures/usage_768_copilot_kimi_go.json"
    ));
    let input: Vec<serde_json::Value> = evidence.documents("input").as_array().unwrap().clone();
    let records = evidence.records();
    assert_eq!(records.len(), 89, "complete Copilot/Kimi corpus");
    let expected: BTreeSet<_> = input.iter().map(|r| r["id"].as_str().unwrap()).collect();
    let observed: BTreeSet<_> = records.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(expected.len(), 89, "distinct source cases");
    assert_eq!(observed, expected, "exact Copilot/Kimi case coverage");
    let mut full = 0;
    let mut gated = 0;
    let mut diverged = 0;
    for record in &records {
        prepare(record);
        let gate = record["gated"].as_bool().unwrap();
        let mut before = BTreeMap::new();
        for (path, hash) in record["file_sha256"].as_object().unwrap() {
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                hash.as_str().unwrap()
            );
            before.insert(path, bytes);
        }
        if gate {
            gated += 1;
        } else {
            let provider = match record["provider"].as_str().unwrap() {
                "copilot" => super::copilot(),
                "kimi" => super::kimi(),
                _ => panic!("source provider"),
            };
            let transport = Arc::new(Unauthorized(Mutex::new(Vec::new())));
            let report = Service::with_transport(vec![provider], transport.clone()).report();
            let actual = serde_json::to_value(report).unwrap();
            let mut expected = record["report"].clone();
            // The fresh Go process uses the immutable pre-#930 revision.
            // Preserve its diagnostic and every unrelated field verbatim.
            for provider in expected["providers"].as_array_mut().expect("providers") {
                if let Some(detail) = provider["auth_status"]["detail"].as_str()
                    && detail.starts_with("credential resolution failed: ")
                {
                    let detail = detail.to_owned();
                    assert_eq!(provider["configured"], false);
                    assert!(provider.get("error").is_none());
                    provider["configured"] = true.into();
                    provider["error"] = detail.into();
                }
            }
            diverged += usize::from(apply_accepted_divergence(
                record["id"].as_str().unwrap(),
                &mut expected,
            ));
            assert_eq!(
                actual, expected,
                "{}: complete Copilot/Kimi report",
                record["id"]
            );
            assert_eq!(
                serde_json::to_value(transport.0.lock().unwrap().clone()).unwrap(),
                normalized_requests(&record["requests"]),
                "{}: complete request bytes",
                record["id"]
            );
            full += 1;
        }
        for (path, bytes) in before {
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "{}: read-only",
                record["id"]
            );
        }
    }
    assert_eq!(gated, 7);
    assert_eq!(diverged, ACCEPTED_DIVERGENCES.len(), "accepted divergences");
    assert_eq!(full + gated, 89);
    baseline_routes(&evidence, &input);
    owner_matches_frozen_go(&evidence);
}

// Original cases outside this increment get routing proof, not full native parity.
fn baseline_routes(evidence: &frozen::Frozen, current: &[serde_json::Value]) {
    let original: Vec<serde_json::Value> = evidence
        .documents("baseline_input")
        .as_array()
        .unwrap()
        .clone();
    let oracle = evidence.documents("baseline");
    let records = oracle["records"].as_array().unwrap();
    assert_eq!(original.len(), 97);
    assert_eq!(records.len(), 97);
    let ids: BTreeSet<_> = original.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 97);
    assert_eq!(
        ids,
        records.iter().map(|r| r["id"].as_str().unwrap()).collect()
    );
    let mut mapping = Vec::new();
    let mut retained = 0;
    for source in &original {
        let id = source["id"].as_str().unwrap();
        if let Some(row) = current.iter().find(|r| r["id"] == source["id"]) {
            for field in ["id", "provider", "files", "env", "home_mode"] {
                assert_eq!(
                    source[field], row[field],
                    "{id}: exact retained original input"
                );
            }
            retained += 1;
            continue;
        }
        let record = records.iter().find(|r| r["id"] == source["id"]).unwrap();
        prepare(record);
        let before: BTreeMap<_, _> = record["file_sha256"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, hash)| {
                let bytes = std::fs::read(path).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    hash.as_str().unwrap()
                );
                (path, bytes)
            })
            .collect();
        let family = if id.contains("-base-") {
            "custom base: existing bounded URL contract"
        } else if id.contains("-workspace-") {
            "workspace: existing cookie-dependent contract"
        } else {
            assert!(
                id.starts_with("nous-jwt-"),
                "{id}: explicitly mapped family"
            );
            "JWT expiry: existing architecture gate or missing/expired credential"
        };
        for (path, bytes) in before {
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "{id}: original route read-only"
            );
        }
        mapping.push(serde_json::json!({"id":id, "provider":source["provider"],
            "family":family, "read_only":true,
            "proof":"fresh original Go constructor plus native eligibility only; no full native report/request parity assertion"}));
    }
    assert_eq!(retained, 66);
    assert_eq!(mapping.len(), 31);
}
