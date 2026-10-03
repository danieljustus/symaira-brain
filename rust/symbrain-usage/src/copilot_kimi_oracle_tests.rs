//! Fresh actual Go constructors and complete synthetic request comparisons.
use crate::{Request, Response, Service, Transport};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

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
    }
    requests
}
#[test]
#[ignore = "requires fresh actual immutable Go process evidence"]
fn copilot_kimi_oracle_matches_fresh_go() {
    let fixture = std::env::var("USAGE_COPILOT_KIMI_ORACLE").expect("Go Copilot/Kimi evidence");
    let input = std::env::var("USAGE_COPILOT_KIMI_INPUT").expect("source input");
    let input: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let records: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture).expect("Go Copilot/Kimi evidence")).unwrap();
    assert_eq!(records.len(), 86, "complete Copilot/Kimi corpus");
    let expected: BTreeSet<_> = input.iter().map(|r| r["id"].as_str().unwrap()).collect();
    let observed: BTreeSet<_> = records.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(expected.len(), 86, "distinct source cases");
    assert_eq!(observed, expected, "exact Copilot/Kimi case coverage");
    let mut full = 0;
    let mut gated = 0;
    for record in &records {
        prepare(record);
        let gate = record["gated"].as_bool().unwrap();
        assert_eq!(
            super::needs_go_fallback(),
            gate,
            "{}: conservative native eligibility",
            record["id"]
        );
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
            assert_eq!(
                serde_json::to_value(report).unwrap(),
                record["report"],
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
    assert_eq!(gated, if cfg!(windows) { 6 } else { 5 });
    if let Ok(path) = std::env::var("USAGE_COPILOT_KIMI_NATIVE") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"cases":86,"passed":86,"failed":0,"full_reports":full,"gated":gated})).unwrap()).unwrap();
    }
}
