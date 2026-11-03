//! Go-derived oracle for the former Go-fallback gates of `symbrain usage` (#768).
//!
//! `scripts/usage-native-gates-oracle` records what the shipped Go provider
//! constructors, `BuildReport` and net/http request validation produce for each
//! case in a disposable HOME. This test recreates the same tree and runs the
//! native production constructors and `Service` in a child process with a
//! cleared environment, so process-global `HOME`/`cwd` are never mutated in the
//! test binary. Provider HTTP uses `FixtureTransport` and the Claude Keychain is
//! an injected fake: no live endpoint, no real Keychain, no operator state.
//! Every case must reproduce one of Go's observed outcomes byte for byte.

use super::{ProviderSelection, selected_providers_with_claude_keychain};
use crate::{FixtureTransport, Response, Service};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// Header values (the Kimi platform label) and the file-access semantics of the
// reference implementation are per OS, so each supported OS replays the fixture
// the oracle produced there.
#[cfg(target_os = "macos")]
const ORACLE: &str = include_str!("../tests/fixtures/usage_native_gates_darwin.json");
#[cfg(target_os = "linux")]
const ORACLE: &str = include_str!("../tests/fixtures/usage_native_gates_linux.json");
const CASE_ENV: &str = "SYMBRAIN_GATES_CASE";
const OUT_ENV: &str = "SYMBRAIN_GATES_OUT";
// Go draws a random server-instance UUID per request; the native constant is a
// documented, Go-possible value. Hostname and runtime version are host facts.
const DYNAMIC_HEADERS: [&str; 4] = [
    "x-msh-device-name",
    "x-msh-os-version",
    "x-msh-device-model",
    "x-server-instance",
];

fn child_test_name() -> String {
    let module = module_path!();
    let local = module.split_once("::").map_or(module, |(_, rest)| rest);
    format!("{local}::native_gates_child")
}

/// Child-process entry point; a no-op in the ordinary test run.
#[test]
fn native_gates_child() {
    let (Some(case_path), Some(out_path)) =
        (std::env::var_os(CASE_ENV), std::env::var_os(OUT_ENV))
    else {
        return;
    };
    let case: Value = serde_json::from_slice(&fs::read(case_path).expect("case")).expect("json");
    let ids: Vec<&str> = case["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("id"))
        .collect();
    let selection = ProviderSelection::parse(&ids.join(",")).expect("selection");
    let keychain_calls = std::cell::Cell::new(0_u32);
    let keychain = case["keychain"].as_str().unwrap_or_default().to_owned();
    let expires = case["keychain_expires_ms"].as_u64();
    let providers = selected_providers_with_claude_keychain(&selection, || {
        keychain_calls.set(keychain_calls.get() + 1);
        (!keychain.is_empty()).then(|| {
            (
                keychain.clone(),
                expires.map(|ms| SystemTime::UNIX_EPOCH + Duration::from_millis(ms)),
            )
        })
    });
    let status = u16::try_from(case["status"].as_u64().filter(|s| *s != 0).unwrap_or(401))
        .expect("status");
    let responses: BTreeMap<String, Response> = ids
        .iter()
        .map(|id| {
            (
                (*id).to_owned(),
                Response {
                    status,
                    headers: BTreeMap::new(),
                    body: b"{}".to_vec(),
                },
            )
        })
        .collect();
    let transport = Arc::new(FixtureTransport::new(responses));
    let report = Service::with_transport(providers, transport.clone()).report();
    let requests: Vec<Value> = transport
        .requests()
        .into_iter()
        .map(|request| {
            let uri = crate::go_url::wire_url(&request.url)
                .ok_or_else(|| "unsendable URL".to_owned())
                .and_then(|wire| ureq::http::Uri::try_from(wire.as_str()).map_err(|e| e.to_string()));
            let raw_headers: BTreeMap<String, String> = request
                .header_bytes
                .iter()
                .map(|(name, bytes)| {
                    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                    (name.to_ascii_lowercase(), hex)
                })
                .collect();
            let headers: BTreeMap<String, String> = request
                .headers
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
                .filter(|(name, _)| !raw_headers.contains_key(&name.to_ascii_lowercase()))
                .map(|(name, value)| {
                    let name = name.to_ascii_lowercase();
                    let value = if DYNAMIC_HEADERS.contains(&name.as_str()) {
                        "<dynamic>".to_owned()
                    } else {
                        value.clone()
                    };
                    (name, value)
                })
                .collect();
            json!({
                "method": request.method,
                "url": request.url,
                "host": uri.as_ref().ok().and_then(|u| u.authority()).map(ToString::to_string),
                "request_uri": uri.as_ref().ok().and_then(|u| u.path_and_query()).map(ToString::to_string),
                "uri_error": uri.as_ref().err().map(ToString::to_string),
                "headers": headers,
                "raw_headers": raw_headers,
                "body": String::from_utf8_lossy(&request.body.unwrap_or_default()),
            })
        })
        .collect();
    let observed = json!({
        "report": serde_json::to_value(&report).expect("report"),
        "requests": requests,
        "keychain_calls": keychain_calls.get(),
    });
    fs::write(out_path, serde_json::to_vec(&observed).expect("encode")).expect("write result");
}

fn apply(home: &Path, files: &[Value]) {
    let expand = |value: &str| value.replace("$HOME", home.to_str().expect("UTF-8 root"));
    for entry in files {
        let path = home.join(entry["path"].as_str().expect("path"));
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        match entry["kind"].as_str().unwrap_or("file") {
            "dir" => fs::create_dir_all(&path).expect("dir"),
            "symlink" => std::os::unix::fs::symlink(
                expand(entry["target"].as_str().expect("target")),
                &path,
            )
            .expect("symlink"),
            _ => {
                let mut contents = entry["contents_hex"].as_str().map_or_else(
                    || entry["contents"].as_str().unwrap_or_default().as_bytes().to_vec(),
                    |hex| {
                        (0..hex.len())
                            .step_by(2)
                            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex"))
                            .collect()
                    },
                );
                let pad = usize::try_from(entry["pad_to"].as_u64().unwrap_or(0)).expect("pad");
                if pad > contents.len() {
                    contents.resize(pad, b' ');
                }
                fs::write(&path, contents).expect("file");
            }
        }
    }
}

fn run_case(case: &Value, scratch: &Path) -> Value {
    let home = scratch.join("home");
    fs::create_dir_all(&home).expect("home");
    apply(&home, case["files"].as_array().map_or(&[][..], Vec::as_slice));
    let case_path = scratch.join("case.json");
    let out_path = scratch.join("out.json");
    fs::write(&case_path, serde_json::to_vec(case).expect("case")).expect("case file");
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", &child_test_name(), "--test-threads=1", "--nocapture"])
        .env_clear()
        .current_dir(&home)
        .env(CASE_ENV, &case_path)
        .env(OUT_ENV, &out_path);
    for (name, value) in case["env"].as_object().into_iter().flatten() {
        command.env(name, value.as_str().expect("env value").replace("$HOME", home.to_str().expect("UTF-8")));
    }
    match case["home"].as_str().unwrap_or("set") {
        "unset" => {}
        "empty" => {
            command.env("HOME", "");
        }
        _ => {
            command.env("HOME", &home);
        }
    }
    let output = command.output().expect("child");
    assert!(
        output.status.success(),
        "child failed for {}: {}",
        case["id"],
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&fs::read(&out_path).expect("child result")).expect("child json")
}

fn sorted_requests(requests: &Value) -> Vec<Value> {
    let mut list = requests.as_array().cloned().unwrap_or_default();
    list.sort_by_key(ToString::to_string);
    list
}

/// Go's `req.URL.Host`/`RequestURI` against the native URL parsed by the HTTP
/// stack that actually sends it. Returns a description of the first mismatch.
fn request_mismatch(go: &Value, native: &Value) -> Option<String> {
    for field in ["method", "url", "headers", "raw_headers", "body"] {
        if go[field] != native[field] {
            return Some(format!("{field}: go {} native {}", go[field], native[field]));
        }
    }
    if native["uri_error"].is_string() {
        return Some(format!("native URI rejected {}: {}", native["url"], native["uri_error"]));
    }
    // Go's recorded host is pre-IDNA; compare the ASCII form the wire carries.
    let go_host = go["host"].as_str().and_then(|host| {
        crate::go_url::wire_url(&format!("https://{}", host.replace('%', "%25").chars().map(|c| if c.is_ascii() { c.to_string() } else { c.to_string().bytes().map(|b| format!("%{b:02X}")).collect() }).collect::<String>()))
    });
    let go_authority = go_host.as_deref().and_then(|url| url.strip_prefix("https://")).map(str::to_owned);
    let native_authority = native["host"].as_str().map(str::to_owned);
    if go_authority != native_authority || go["request_uri"] != native["request_uri"] {
        return Some(format!(
            "target: go {} {} native {} {} (urls {} / {})",
            go["host"], go["request_uri"], native["host"], native["request_uri"], go["url"], native["url"]
        ));
    }
    None
}

fn observed_mismatch(go: &Value, native: &Value) -> Option<String> {
    if go["report"] != native["report"] {
        return Some(format!("report: go {} native {}", go["report"], native["report"]));
    }
    if go["keychain_calls"] != native["keychain_calls"] {
        return Some(format!(
            "keychain calls: go {} native {}",
            go["keychain_calls"], native["keychain_calls"]
        ));
    }
    let (go_requests, native_requests) = (sorted_requests(&go["requests"]), sorted_requests(&native["requests"]));
    if go_requests.len() != native_requests.len() {
        let urls = |list: &[Value]| list.iter().map(|request| request["url"].to_string()).collect::<Vec<_>>().join(" ");
        return Some(format!(
            "request count: go {} [{}] native {} [{}]",
            go_requests.len(),
            urls(&go_requests),
            native_requests.len(),
            urls(&native_requests)
        ));
    }
    // Both lists are sorted by their own serialization, so pair by method+headers.
    for go_request in &go_requests {
        let found = native_requests.iter().any(|candidate| request_mismatch(go_request, candidate).is_none());
        if !found {
            let first = native_requests.first().and_then(|candidate| request_mismatch(go_request, candidate));
            return Some(format!("request: {}", first.unwrap_or_default()));
        }
    }
    None
}

#[test]
fn native_reports_match_go_for_every_former_gate() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("oracle");
    let cases = oracle["cases"].as_array().expect("cases");
    assert!(cases.len() >= 400, "oracle lost cases: {}", cases.len());
    let filter = std::env::var("SYMBRAIN_GATES_FILTER").ok();
    let mut failures = Vec::new();
    let mut executed = 0;
    for case in cases {
        let id = case["id"].as_str().expect("id");
        if filter.as_deref().is_some_and(|needle| !id.contains(needle)) {
            continue;
        }
        let scratch = tempfile::tempdir().expect("scratch");
        let native = run_case(case, scratch.path());
        executed += 1;
        let observed = case["observed"].as_array().expect("observed");
        let mismatches: Vec<String> = observed
            .iter()
            .filter_map(|go| observed_mismatch(go, &native))
            .collect();
        if mismatches.len() == observed.len() {
            failures.push(format!("{id}: {}", mismatches.first().cloned().unwrap_or_default()));
        }
    }
    assert!(filter.is_some() || executed == cases.len());
    assert!(failures.is_empty(), "{} of {executed} cases differ:\n{}", failures.len(), failures.join("\n"));
}
