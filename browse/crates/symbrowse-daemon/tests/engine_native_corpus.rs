//! ENG-009 / ENG-010: one native navigation corpus run against each explicitly
//! requested engine through the production daemon. The corpus drives a loopback
//! fixture server (redirect, JavaScript, cookies, storage, response capture,
//! timeout) and proves that the engine
//!
//! - is the one that was requested and reports its own detected version,
//! - uses its own network stack and cookie jar (the `Cookie`/`User-Agent` the
//!   server saw are the browser's),
//! - keeps its own capability truth (unsupported operations fail with the
//!   typed `unsupported` code), and
//! - leaves no owned process behind.
//!
//! Chrome and Firefox run under `SYMBROWSE_E2E=1`. Safari runs only when the
//! user's normal Safari is not running; otherwise the case is skipped with a
//! recorded reason and Safari is never quit, killed or automated.
#![cfg(unix)]
#![deny(unsafe_code)]

mod common;

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use common::corpus_fixture;
use serde_json::{Value, json};
use symbrowse_daemon::{Frame, Server, ServerOptions, SessionSpec};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);

fn raw_request(socket: &Path, command: &str, args: Value) -> Value {
    let mut stream = UnixStream::connect(socket).expect("connect daemon");
    let frame = Frame {
        cmd: command.into(),
        args: Some(args),
        session: "native-corpus".into(),
        ..Frame::default()
    };
    writeln!(stream, "{}", serde_json::to_string(&frame).expect("encode")).expect("write");
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("read response");
    serde_json::from_str(&response).expect("decode response")
}

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn case(id: &str, engine: &str) {
    println!("PARITY_CASE {id}.{engine} PASS");
}

/// What each engine honestly supports; the corpus asserts both directions.
struct Engine {
    /// `SessionSpec.engine` value requested from the daemon.
    requested: &'static str,
    /// Kind the engine reports in `capabilities` / `engine.info`.
    kind: &'static str,
    /// Substring of the engine's own `User-Agent`.
    user_agent: &'static str,
    response_capture: bool,
    file_transfer: bool,
}

const CHROME: Engine = Engine {
    requested: "chrome",
    kind: "chrome",
    user_agent: "Chrome/",
    response_capture: true,
    file_transfer: true,
};

const FIREFOX: Engine = Engine {
    requested: "firefox",
    kind: "firefox",
    user_agent: "Firefox/",
    response_capture: true,
    file_transfer: false,
};

struct Daemon {
    socket: PathBuf,
    server: Arc<Server>,
    thread: Option<thread::JoinHandle<()>>,
    root: PathBuf,
}

impl Daemon {
    fn start(engine: &str, root: &Path, executable: Option<&Path>) -> Self {
        let mut spec = SessionSpec::for_session("native-corpus");
        spec.engine = engine.into();
        spec.state_dir = root.join("state");
        spec.cache_dir = root.join("cache");
        // Unix socket paths are limited to ~100 bytes; TMPDIR may be longer.
        spec.socket_path = PathBuf::from(format!("/tmp/sbec-{}-{engine}.sock", std::process::id()));
        spec.operation_timeout = OPERATION_TIMEOUT;
        spec.idle_timeout = Some(Duration::from_secs(120));
        // The loopback fixture is a private address.
        spec.allow_private = true;
        if let Some(executable) = executable {
            spec.executable_path = executable.to_path_buf();
        }
        let server = Arc::new(
            Server::new(ServerOptions {
                session_spec: Some(spec.clone()),
                operation_timeout: OPERATION_TIMEOUT,
                idle_timeout: Some(Duration::from_secs(120)),
                state_key_sources: Some(common::isolated_key_sources()),
                ..ServerOptions::default()
            })
            .expect("create daemon"),
        );
        let socket = spec.socket_path.clone();
        let serving = Arc::clone(&server);
        let thread = thread::spawn(move || serving.listen_and_serve().expect("serve daemon"));
        for _ in 0..200 {
            if socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(socket.exists(), "daemon socket did not appear");
        Self {
            socket,
            server,
            thread: Some(thread),
            root: root.to_path_buf(),
        }
    }

    fn request(&self, command: &str, args: Value) -> Value {
        raw_request(&self.socket, command, args)
    }

    /// Send a `confirm`-class command (`eval`) and approve its out-of-band
    /// prompt the way a human would through `oob.complete`: the production
    /// gate path, not a bypass.
    fn request_approved(&self, command: &str, args: Value) -> Value {
        let socket = self.socket.clone();
        let command = command.to_owned();
        let worker = thread::spawn(move || raw_request(&socket, &command, args));
        for _ in 0..200 {
            if worker.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
            let status = self.request("oob.status", json!({}));
            if let Some(id) = status["data"]["prompt"]["id"].as_str() {
                let done = self.request("oob.complete", json!({"id": id}));
                assert_eq!(done["success"], true, "oob.complete: {done}");
                break;
            }
        }
        worker.join().expect("approval worker")
    }

    fn eval(&self, expression: &str) -> Value {
        let response = self.request_approved("eval", json!({"expression": expression}));
        assert_eq!(response["success"], true, "eval {expression:?}: {response}");
        response["data"].clone()
    }

    fn ok(&self, command: &str, args: Value) -> Value {
        let response = self.request(command, args);
        assert_eq!(response["success"], true, "{command}: {response}");
        response["data"].clone()
    }

    /// Stop the daemon and prove no process of this corpus survives.
    fn stop_and_assert_clean(mut self) {
        self.server.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let marker = self.root.to_string_lossy().into_owned();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let leftover = owned_processes(&marker);
            if leftover.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "engine processes survived daemon stop: {leftover:?}"
            );
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Daemon {
    /// A failing assertion must not leave an engine process behind.
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.server.stop();
            let _ = thread.join();
        }
    }
}

/// Command lines that mention the corpus' private root: the engine's own
/// profile, never a foreign browser or the user's Safari.
fn owned_processes(marker: &str) -> Vec<String> {
    let table = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&table.stdout)
        .lines()
        .filter(|line| line.contains(marker))
        .map(|line| line.chars().take(200).collect())
        .collect()
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(map) => ["value", "url", "title", "text"]
            .iter()
            .find_map(|key| map.get(*key))
            .map(scalar)
            .unwrap_or_default(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn find_cookie<'a>(cookies: &'a Value, name: &str) -> Option<&'a Value> {
    cookies["cookies"]
        .as_array()?
        .iter()
        .find(|cookie| cookie["name"] == name)
}

fn version_output(executable: &Path) -> String {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .expect("executable --version");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_corpus(engine: &Engine, executable: &Path) {
    let name = engine.requested;
    let Some(root) = common::isolated_environment_root(
        &format!("symbrowse-corpus-{name}"),
        &format!("corpus_{name}"),
    ) else {
        return;
    };
    let fixture = corpus_fixture::serve();
    let daemon = Daemon::start(name, &root, Some(executable));

    // ENG-009: the daemon reports the explicit selection, and the engine
    // reports itself (own version, own capability partition).
    let status = daemon.ok("daemon.status", json!({}));
    assert_eq!(status["engine"], name, "{status}");
    assert_eq!(status["mode"], "browser", "{status}");
    let info = daemon.ok("engine.info", json!({}));
    assert_eq!(info["engine"], name, "{info}");
    assert_eq!(info["kind"], engine.kind, "{info}");
    let version = info["version"].as_str().expect("detected version");
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "{info}"
    );
    assert!(
        version_output(executable).contains(version),
        "engine reported {version}, executable says {:?}",
        version_output(executable)
    );
    assert_eq!(info["capabilities"]["kind"], engine.kind, "{info}");
    let rendered = info.to_string();
    for forbidden in [
        "user_data_dir",
        "profile",
        root.to_str().expect("utf-8 root"),
    ] {
        assert!(
            !rendered.contains(forbidden),
            "engine.info leaks {forbidden}"
        );
    }
    case("ENG-009.selected-engine-version", name);

    // Redirect + JavaScript: the page's own script ran in the engine.
    let opened = daemon.request("open", json!({"url": fixture.url("/redirect")}));
    assert_eq!(opened["success"], true, "open redirect: {opened}");
    let url = scalar(&daemon.ok("get.url", json!({})));
    assert!(url.ends_with("/landing"), "redirect did not land: {url}");
    assert_eq!(scalar(&daemon.ok("get.title", json!({}))), "landing");
    case("ENG-009.redirect", name);
    let sum = daemon.eval("1 + 2");
    assert_eq!(sum["value"], 3, "eval: {sum}");
    let ready = daemon.eval("document.body.dataset.ready");
    assert_eq!(ready["value"], "yes", "page script did not run: {ready}");
    let thrown = daemon.request_approved("eval", json!({"expression": "throw new Error('probe')"}));
    let failed = thrown["success"] == false
        || thrown["data"]["exception_text"]
            .as_str()
            .is_some_and(|text| !text.is_empty());
    assert!(failed, "a script exception must be reported: {thrown}");
    case("ENG-009.javascript", name);

    // Cookies: the HTTP Set-Cookie from the redirect and the script cookie are
    // in the engine's own jar; a cookie set through the daemon is sent by the
    // engine's own network stack on the next request.
    let cookies = daemon.ok("cookies.list", json!({}));
    assert_eq!(
        find_cookie(&cookies, "session").map(|c| c["value"].clone()),
        Some(json!("abc")),
        "HTTP cookie missing: {cookies}"
    );
    assert_eq!(
        find_cookie(&cookies, "js").map(|c| c["value"].clone()),
        Some(json!("1")),
        "script cookie missing: {cookies}"
    );
    daemon.ok(
        "cookies.set",
        json!({"cookie": {"name": "manual", "value": "1"}, "url": fixture.url("/")}),
    );
    daemon.ok("open", json!({"url": fixture.url("/echo")}));
    let echoed = scalar(&daemon.ok("get.text", json!({"selector": "body"})));
    for expected in ["session=abc", "js=1", "manual=1"] {
        assert!(echoed.contains(expected), "{expected} not sent: {echoed}");
    }
    assert!(
        echoed.contains(engine.user_agent),
        "request was not made by the {name} network stack: {echoed}"
    );
    daemon.ok(
        "cookies.clear",
        json!({"name": "manual", "url": fixture.url("/")}),
    );
    let after = daemon.ok("cookies.list", json!({}));
    assert!(find_cookie(&after, "manual").is_none(), "cleared: {after}");
    case("ENG-009.cookies", name);

    // Storage written by the page's script, then through the daemon.
    daemon.ok("open", json!({"url": fixture.url("/landing")}));
    let local = daemon.ok("storage.list", json!({"kind": "local"}));
    assert_eq!(local["items"]["k"], "v", "local storage: {local}");
    let session = daemon.ok("storage.list", json!({"kind": "session"}));
    assert_eq!(session["items"]["s"], "1", "session storage: {session}");
    daemon.ok(
        "storage.set",
        json!({"kind": "local", "key": "from", "value": "daemon"}),
    );
    let local = daemon.ok("storage.list", json!({"kind": "local"}));
    assert_eq!(local["items"]["from"], "daemon", "{local}");
    daemon.ok("storage.clear", json!({"kind": "local"}));
    let local = daemon.ok("storage.list", json!({"kind": "local"}));
    assert_eq!(local["items"].as_object().map(|items| items.len()), Some(0));
    case("ENG-009.storage", name);

    // Response capture: the browser's own view of the traffic.
    let capabilities = daemon.ok("capabilities", json!({}));
    let has = |interface: &str| {
        capabilities["interfaces"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == interface))
    };
    assert_eq!(
        has("NetworkEvents"),
        engine.response_capture,
        "{capabilities}"
    );
    assert_eq!(has("FileTransfer"), engine.file_transfer, "{capabilities}");
    if engine.response_capture {
        daemon.ok("network.capture", json!({}));
        daemon.ok("open", json!({"url": fixture.url("/json")}));
        let captured = daemon.ok("network.requests", json!({}));
        let seen = captured["requests"].as_array().expect("requests");
        assert!(
            seen.iter().any(|request| {
                request["url"]
                    .as_str()
                    .is_some_and(|u| u.ends_with("/json"))
                    && request["status"] == 200
            }),
            "response capture missed /json: {captured}"
        );
        case("ENG-009.response-capture", name);
    }

    // Capability truth: what the engine does not implement fails explicitly.
    if !engine.file_transfer {
        for command in ["download", "downloads.list", "upload", "network.har"] {
            // Download/upload are `confirm`-class: the policy gate runs first
            // and a human approves, then the engine answers for itself.
            let response = daemon.request_approved(command, json!({}));
            assert_eq!(response["success"], false, "{command}: {response}");
            assert_eq!(
                response["error"]["code"], "unsupported",
                "{command}: {response}"
            );
        }
    } else {
        daemon.ok("downloads.list", json!({}));
    }
    case("ENG-009.capability-truth", name);

    // Timeout: a server that never answers ends in the typed timeout, the
    // daemon stays up and still reports the same engine.
    let started = Instant::now();
    let hung = daemon.request("open", json!({"url": fixture.url("/hang")}));
    assert_eq!(hung["success"], false, "{hung}");
    assert_eq!(hung["error"]["code"], "operation_timeout", "{hung}");
    assert!(
        started.elapsed() < OPERATION_TIMEOUT + Duration::from_secs(10),
        "timeout took {:?}",
        started.elapsed()
    );
    let status = daemon.ok("daemon.status", json!({}));
    assert_eq!(status["engine"], name);
    case("ENG-009.timeout", name);

    assert!(
        fixture.requests().iter().any(|line| line == "GET /hang"),
        "the hang request never reached the fixture"
    );
    daemon.stop_and_assert_clean();
    case("ENG-009.cleanup", name);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corpus_chrome() {
    if !enabled() {
        return;
    }
    let executable = symbrowse_engine_chrome::discover_chrome_executable()
        .expect("native Chrome corpus needs a local Chrome");
    run_corpus(&CHROME, &executable);
}

#[test]
fn corpus_firefox() {
    if !enabled() {
        return;
    }
    let executable = symbrowse_engine_firefox::resolve_firefox_executable(None)
        .expect("native Firefox corpus needs a local Firefox");
    run_corpus(&FIREFOX, &executable);
}

/// An explicitly requested engine that is not installed fails with the typed
/// `engine_unavailable` code. The daemon keeps reporting the requested engine;
/// no other engine or static mode answers and nothing is launched.
#[test]
fn unavailable_engine_is_a_typed_error_without_substitution() {
    let Some(root) = common::isolated_environment_root(
        "symbrowse-corpus-unavailable",
        "unavailable_engine_is_a_typed_error_without_substitution",
    ) else {
        return;
    };
    for name in ["chrome", "firefox"] {
        let missing = root.join("no-such-browser");
        let daemon = Daemon::start(name, &root, Some(&missing));
        for command in ["engine.info", "open"] {
            let response = daemon.request(command, json!({"url": "http://127.0.0.1:9/"}));
            assert_eq!(response["success"], false, "{name} {command}: {response}");
            assert_eq!(
                response["error"]["code"], "engine_unavailable",
                "{name} {command}: {response}"
            );
            let message = response["error"]["message"].as_str().unwrap_or_default();
            assert!(message.contains(name), "{message}");
            assert!(
                response["error"]["hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("no other engine")),
                "{response}"
            );
        }
        let status = daemon.ok("daemon.status", json!({}));
        assert_eq!(status["engine"], name);
        daemon.stop_and_assert_clean();
        case("ENG-009.unavailable-typed", name);
    }
    let _ = fs::remove_dir_all(root);
}

/// `engine.info` for Safari reads the installed application's bundle version
/// and never launches, attaches to or quits Safari (no automation Safari
/// appears), so it is safe next to the user's normal Safari.
#[cfg(target_os = "macos")]
#[test]
fn safari_info_reports_the_installed_version_without_touching_safari() {
    let Some(root) = common::isolated_environment_root(
        "symbrowse-corpus-safari-info",
        "safari_info_reports_the_installed_version_without_touching_safari",
    ) else {
        return;
    };
    let automation = || {
        let table = Command::new("ps")
            .args(["-axo", "pid=,command="])
            .output()
            .expect("ps");
        String::from_utf8_lossy(&table.stdout)
            .lines()
            .filter(|line| line.contains("/Safari ") && line.contains("--automation"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let before = automation();
    let daemon = Daemon::start("safari-bidi", &root, None);
    let info = daemon.ok("engine.info", json!({}));
    assert_eq!(info["engine"], "safari-bidi", "{info}");
    assert_eq!(info["kind"], "safari", "{info}");
    let version = info["version"].as_str().expect("installed Safari version");
    let plist = Command::new("/usr/bin/defaults")
        .args([
            "read",
            "/Applications/Safari.app/Contents/Info",
            "CFBundleShortVersionString",
        ])
        .output()
        .expect("defaults");
    let system = Command::new("/usr/bin/defaults")
        .args([
            "read",
            "/System/Cryptexes/App/System/Applications/Safari.app/Contents/Info",
            "CFBundleShortVersionString",
        ])
        .output()
        .expect("defaults");
    assert!(
        [plist, system]
            .iter()
            .any(|out| String::from_utf8_lossy(&out.stdout).trim() == version),
        "reported {version}"
    );
    assert_eq!(info["capabilities"]["kind"], "safari-bidi", "{info}");
    assert_eq!(automation(), before, "engine.info must not start Safari");
    daemon.stop_and_assert_clean();
    case("ENG-009.safari-info", "safari-bidi");
    let _ = fs::remove_dir_all(root);
}

/// Safari is exercised only through the isolated automation path and only when
/// the user's normal Safari is not running. A running normal Safari is never
/// quit, killed or automated: the case is skipped with a recorded reason.
#[cfg(target_os = "macos")]
#[test]
fn corpus_safari_guarded() {
    if !enabled() {
        return;
    }
    let table = Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .expect("ps");
    let normal_safari = String::from_utf8_lossy(&table.stdout).lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|exe| exe.ends_with("/Safari") && !line.contains("--automation"))
    });
    if normal_safari {
        println!(
            "PARITY_CASE ENG-009.safari SKIPPED normal Safari is running; refusing to automate the user's session"
        );
        return;
    }
    let Some(root) =
        common::isolated_environment_root("symbrowse-corpus-safari", "corpus_safari_guarded")
    else {
        return;
    };
    let fixture = corpus_fixture::serve();
    let daemon = Daemon::start("safari-bidi", &root, None);
    let info = daemon.ok("engine.info", json!({}));
    assert_eq!(info["engine"], "safari-bidi", "{info}");
    assert_eq!(info["kind"], "safari", "{info}");
    assert!(
        info["version"].as_str().is_some_and(|v| !v.is_empty()),
        "{info}"
    );
    let opened = daemon.request("open", json!({"url": fixture.url("/redirect")}));
    assert_eq!(opened["success"], true, "{opened}");
    assert_eq!(scalar(&daemon.ok("get.title", json!({}))), "landing");
    case("ENG-009.safari-redirect", "safari-bidi");
    daemon.stop_and_assert_clean();
    let _ = fs::remove_dir_all(root);
}
