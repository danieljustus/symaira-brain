//! Replays the frozen Go daemon oracle (`fixtures/daemon_oracle/go_chrome.json`,
//! captured by `capture.py` from the Go `symbrowse daemon` with real headless
//! Chrome) against the Rust daemon and compares response JSON step by step.
//!
//! The Rust side needs no Go.  It does need real Chrome, so the test only runs
//! when `SYMBROWSE_LIVE_CHROME=1` (and `SYMBROWSE_CHROME_EXECUTABLE`/the default
//! macOS Chrome is installed).  Steps listed in `KNOWN_GAPS` are documented
//! unimplemented Go-parity commands: they must still differ (so a fixed
//! command forces the entry to be removed) and are reported, never hidden.
#![deny(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

mod common;

use serde_json::{Value, json};
use symbrowse_daemon::{Server, ServerOptions, SessionSpec};

/// Scenario steps whose Go behavior the Rust daemon does not reproduce yet.
const KNOWN_GAPS: &[&str] = &[
    // auth.login needs the symvault credential path (vault.go) - not ported.
    "auth_login_noentry",
];

/// Keys whose values are explicitly nondeterministic (timestamps, ids, pids,
/// paths of the isolated roots).
const NONDETERMINISTIC_KEYS: &[&str] = &[
    "timestamp",
    "started_at",
    "saved_at",
    "expires_at",
    "last_activity",
    "created_at",
    "pid",
    "prompt_id",
    "user_data_dir",
    "origin_path",
    // Byte counts include the fixture server's own response headers.
    "encoded_body_size",
    "bodySize",
    "size",
    "startedDateTime",
];

/// Request headers Chrome derives from its own build and host OS. The oracle
/// was recorded with Chrome 155 on macOS, while CI installs the current Chrome
/// for Testing Stable on macOS and Linux, so their values are compared as
/// present, not verbatim.
const BROWSER_VERSION_HEADERS: &[&str] = &["User-Agent", "sec-ch-ua", "sec-ch-ua-platform"];

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/daemon_oracle")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read fixture")).expect("parse fixture")
}

fn serve_site(site: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let site = site.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_owned();
                let response = match site.get(&path) {
                    Some(entry) => {
                        let body = entry["body"].as_str().unwrap_or_default();
                        let mut head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n",
                            entry["content_type"].as_str().unwrap_or("text/html"),
                            body.len()
                        );
                        for (key, value) in entry["headers"].as_object().into_iter().flatten() {
                            head.push_str(&format!("{key}: {}\r\n", value.as_str().unwrap_or("")));
                        }
                        format!("{head}\r\n{body}")
                    }
                    None => "HTTP/1.1 404 Not Found\r\n\r\n".to_owned(),
                };
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn substitute(value: &Value, base: &str, work: &str) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("{BASE}", base).replace("{WORK}", work)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| substitute(v, base, work)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, base, work)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn send(socket: &Path, frame: &Value) -> Value {
    let mut stream = UnixStream::connect(socket).expect("connect daemon");
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .unwrap();
    writeln!(stream, "{frame}").expect("write frame");
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("read response");
    serde_json::from_str(&response).expect("decode response")
}

/// Canonical form compared on both sides: placeholders, numeric forms and the
/// documented nondeterministic keys.
fn normalize(value: &Value, base: &str, work: &str, state: &str, key: &str) -> Value {
    match value {
        Value::String(text) => {
            if NONDETERMINISTIC_KEYS.contains(&key) {
                return json!("<nondeterministic>");
            }
            if BROWSER_VERSION_HEADERS.contains(&key) {
                return json!("<browser-version>");
            }
            let mut text = text.replace(base, "{BASE}").replace(work, "{WORK}");
            if text.ends_with("/policy.toml") {
                text = "{STATE}/policy.toml".into();
            }
            if !state.is_empty() {
                text = text.replace(state, "{STATE}");
            }
            Value::String(text)
        }
        Value::Number(number) => {
            if NONDETERMINISTIC_KEYS.contains(&key) {
                return json!("<nondeterministic>");
            }
            match number.as_f64() {
                Some(float) if float.fract() == 0.0 && float.abs() < 1e15 => json!(float as i64),
                _ => value.clone(),
            }
        }
        Value::Array(items) => {
            let mut items: Vec<Value> = items
                .iter()
                .map(|v| normalize(v, base, work, state, key))
                .collect();
            if key == "headers" {
                // HAR header arrays come from an unordered Go map; the
                // fixture servers differ in Date/Server, so those are dropped.
                items.retain(|item| !matches!(item["name"].as_str(), Some("Date" | "Server")));
                items.sort_by_key(|item| item["name"].as_str().unwrap_or_default().to_owned());
                for item in &mut items {
                    if item["name"]
                        .as_str()
                        .is_some_and(|name| BROWSER_VERSION_HEADERS.contains(&name))
                    {
                        item["value"] = json!("<browser-version>");
                    }
                }
            }
            Value::Array(items)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k, normalize(v, base, work, state, k)))
                // A response-header map emptied by dropping Date/Server is
                // the same as an absent one (Go omits empty maps).
                .filter(|(k, v)| {
                    !(k.as_str() == "response_headers"
                        && v.as_object().is_some_and(|m| m.is_empty()))
                })
                .filter(|(k, _)| {
                    // `duration_ms` is omitted by Go when it rounds to 0 ms.
                    k.as_str() != "duration_ms"
                        && !(key == "response_headers" && matches!(k.as_str(), "Date" | "Server"))
                })
                .map(|(k, v)| (k.clone(), v))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Chrome target/frame/request ids are random per run.
fn scrub_ids(step: &str, response: &mut Value) {
    let scrub = |value: &mut Value, keys: &[&str]| {
        fn walk(value: &mut Value, keys: &[&str]) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map.iter_mut() {
                        if keys.contains(&key.as_str()) && child.is_string() {
                            *child = json!("<id>");
                        } else {
                            walk(child, keys);
                        }
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(|item| walk(item, keys)),
                _ => {}
            }
        }
        walk(value, keys);
    };
    if step.starts_with("frame_tree") {
        scrub(response, &["id", "parent_id"]);
    }
    if step.starts_with("network_") {
        scrub(response, &["id"]);
    }
}

/// First differing JSON path, for readable mismatch reports.
fn first_diff(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    match (expected, actual) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                let child = format!("{path}/{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => {
                        if let Some(found) = first_diff(x, y, &child) {
                            return Some(found);
                        }
                    }
                    (x, y) => return Some(format!("{child}: go={x:?} rust={y:?}")),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!(
                    "{path}: go has {} items, rust {}",
                    a.len(),
                    b.len()
                ));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(index, (x, y))| first_diff(x, y, &format!("{path}/{index}")))
        }
        _ if expected == actual => None,
        _ => Some(format!("{path}: go={expected} rust={actual}")),
    }
}

#[test]
fn rust_daemon_matches_frozen_go_oracle() {
    if std::env::var("SYMBROWSE_LIVE_CHROME").as_deref() != Ok("1") {
        eprintln!("SKIP oracle replay: set SYMBROWSE_LIVE_CHROME=1 to run against real Chrome");
        return;
    }
    let oracle = read_json(&fixtures().join("go_chrome.json"));
    let scenarios = read_json(&fixtures().join("scenarios_chrome.json"));
    let site = read_json(&fixtures().join("fixture_site.json"));
    let base = serve_site(site);

    // The daemon reads HOME/XDG at startup; run in a re-executed process whose
    // environment points at an isolated root.
    let Some(root) = common::isolated_environment_root(
        "symbrowse-oracle-replay",
        "rust_daemon_matches_frozen_go_oracle",
    ) else {
        return;
    };
    let session = scenarios["session"].as_str().unwrap_or("oracle").to_owned();
    let mut spec = SessionSpec::for_session(session.clone());
    spec.state_dir = root.join("state");
    spec.cache_dir = root.join("cache");
    // Unix socket paths are limited to ~100 bytes; TMPDIR can be longer.
    spec.socket_path = PathBuf::from(format!("/tmp/sbo-{}.sock", std::process::id()));
    spec.allow_private = true;
    spec.operation_timeout = Duration::from_secs(20);
    spec.idle_timeout = Some(Duration::from_secs(300));
    let socket = spec.socket_path.clone();
    let work = root.to_string_lossy().into_owned();
    let state_dir = spec.state_dir.to_string_lossy().into_owned();

    let server = Arc::new(
        Server::new(ServerOptions {
            socket_path: socket.clone(),
            session: session.clone(),
            session_spec: Some(spec),
            operation_timeout: Duration::from_secs(20),
            idle_timeout: Some(Duration::from_secs(300)),
            ..ServerOptions::default()
        })
        .expect("create daemon"),
    );
    let serving = Arc::clone(&server);
    let handle = thread::spawn(move || serving.listen_and_serve());
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    if !socket.exists() {
        panic!(
            "daemon socket did not appear; server result: {:?}",
            handle.join()
        );
    }

    let results = oracle["results"].as_array().expect("oracle results");
    let steps = scenarios["steps"].as_array().expect("scenario steps");
    assert_eq!(results.len(), steps.len(), "oracle and scenarios disagree");
    let mut mismatches: BTreeMap<String, (Value, Value)> = BTreeMap::new();
    let mut matched = 0usize;
    for (step, recorded) in steps.iter().zip(results) {
        let name = step["name"].as_str().unwrap();
        assert_eq!(recorded["name"], step["name"]);
        let mut frame = substitute(&step["frame"], &base, &work);
        frame["session"] = json!(step["session_override"].as_str().unwrap_or(&session));
        let actual = if step["approve"].as_bool() == Some(true) {
            let socket_for_worker = socket.clone();
            let frame_for_worker = frame.clone();
            let worker = thread::spawn(move || send(&socket_for_worker, &frame_for_worker));
            thread::sleep(Duration::from_millis(400));
            let status = send(&socket, &json!({"cmd": "oob.status", "session": session}));
            if let Some(id) = status["data"]["prompt"]["id"].as_str() {
                send(
                    &socket,
                    &json!({"cmd": "oob.complete", "args": {"id": id}, "session": session}),
                );
            }
            worker.join().expect("approval worker")
        } else {
            if let Some(delay) = step["sleep_before"].as_f64() {
                thread::sleep(Duration::from_secs_f64(delay));
            }
            send(&socket, &frame)
        };
        let mut expected = recorded["response"].clone();
        // The oracle is stored with placeholders already.
        let mut actual_n = normalize(&actual, &base, &work, &state_dir, "");
        let mut expected_n = normalize(&expected, &base, &work, "", "");
        scrub_ids(name, &mut actual_n);
        scrub_ids(name, &mut expected_n);
        expected = expected_n;
        if actual_n == expected {
            matched += 1;
        } else {
            mismatches.insert(name.to_owned(), (expected, actual_n));
        }
    }
    let _ = send(&socket, &json!({"cmd": "daemon.stop", "session": session}));
    let _ = handle.join();
    let _ = fs::remove_dir_all(&root);

    let known: BTreeSet<&str> = KNOWN_GAPS.iter().copied().collect();
    let unexpected: Vec<&String> = mismatches
        .keys()
        .filter(|name| !known.contains(name.as_str()))
        .collect();
    let stale: Vec<&&str> = known
        .iter()
        .filter(|name| !mismatches.contains_key(**name))
        .collect();
    eprintln!(
        "oracle replay: {matched} matched, {} mismatched ({} known gaps) of {} steps",
        mismatches.len(),
        known.len(),
        steps.len()
    );
    for name in &unexpected {
        let (expected, actual) = &mismatches[*name];
        let clip = |value: &Value| value.to_string().chars().take(700).collect::<String>();
        eprintln!(
            "MISMATCH {name} at {}\n  go:   {}\n  rust: {}",
            first_diff(expected, actual, "").unwrap_or_default(),
            clip(expected),
            clip(actual)
        );
    }
    assert!(
        unexpected.is_empty(),
        "unexpected mismatches: {unexpected:?}"
    );
    assert!(
        stale.is_empty(),
        "stale KNOWN_GAPS entries now match: {stale:?}"
    );
}
