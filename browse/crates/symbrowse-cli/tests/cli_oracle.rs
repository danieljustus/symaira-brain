//! Replays the frozen Go CLI observations (`fixtures/cli/cli_oracle.json`,
//! recorded by `fixtures/cli/record_oracle.py` from the Go binary) against the
//! Rust `symbrowse` binary: same argv/stdin/env/files, a recording fake daemon
//! on the session socket, compared on sent frames, stdout, stderr, exit code
//! and written files. No Go is needed at test time.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::Value;

const ORACLE: &str = include_str!("fixtures/cli/cli_oracle.json");

fn is_ts(bytes: &[u8]) -> Option<usize> {
    // dddd-dd-ddTdd:dd:dd(.d+)?(Z|[+-]dd:dd)
    let pattern = b"dddd-dd-ddTdd:dd:dd";
    if bytes.len() < pattern.len() {
        return None;
    }
    for (index, expected) in pattern.iter().enumerate() {
        let ok = if *expected == b'd' {
            bytes[index].is_ascii_digit()
        } else {
            bytes[index] == *expected
        };
        if !ok {
            return None;
        }
    }
    let mut end = pattern.len();
    if bytes.get(end) == Some(&b'.') {
        let digits = bytes[end + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits > 0 {
            end += 1 + digits;
        }
    }
    match bytes.get(end) {
        Some(b'Z') => Some(end + 1),
        Some(b'+' | b'-') => {
            let tail = &bytes[end + 1..];
            (tail.len() >= 5
                && tail[0].is_ascii_digit()
                && tail[1].is_ascii_digit()
                && tail[2] == b':'
                && tail[3].is_ascii_digit()
                && tail[4].is_ascii_digit())
            .then_some(end + 6)
        }
        _ => None,
    }
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

/// Mirrors `mask_text` of `record_oracle.py`.
fn mask(text: &str, kinds: &[String]) -> String {
    let has = |kind: &str| kinds.iter().any(|k| k == kind);
    let mut out = text.to_owned();
    if has("time") {
        let bytes = out.as_bytes().to_vec();
        let mut result = String::new();
        let mut index = 0;
        while index < bytes.len() {
            if let Some(len) = is_ts(&bytes[index..]) {
                result.push_str("<TIMESTAMP>");
                index += len;
            } else {
                let width = out[index..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&out[index..index + width]);
                index += width;
            }
        }
        out = result;
    }
    if has("hex16") {
        let bytes = out.as_bytes().to_vec();
        let mut result = String::new();
        let mut index = 0;
        while index < bytes.len() {
            if is_hex(bytes[index]) {
                let run = bytes[index..].iter().take_while(|b| is_hex(**b)).count();
                if run == 16 {
                    result.push_str("<HEX16>");
                } else {
                    result.push_str(&out[index..index + run]);
                }
                index += run;
            } else {
                let width = out[index..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&out[index..index + width]);
                index += width;
            }
        }
        out = result;
    }
    if has("duration") {
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(position) = rest.find("duration_ms") {
            let (head, tail) = rest.split_at(position + "duration_ms".len());
            result.push_str(head);
            let after_quote = tail.strip_prefix('"').unwrap_or(tail);
            if let Some(after_colon) = after_quote.strip_prefix(':') {
                let value = after_colon.strip_prefix(' ').unwrap_or(after_colon);
                let digits = value.bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 {
                    result.push_str(&tail[..tail.len() - value.len()]);
                    result.push('0');
                    rest = &value[digits..];
                    continue;
                }
            }
            rest = tail;
        }
        result.push_str(rest);
        out = result;
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(position) = rest.find("tokens_est: ") {
            let (head, tail) = rest.split_at(position + "tokens_est: ".len());
            result.push_str(head);
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                result.push('0');
            }
            rest = &tail[digits..];
        }
        result.push_str(rest);
        out = result;
    }
    out
}

struct FakeDaemon {
    stop: Arc<AtomicBool>,
    frames: Arc<Mutex<Vec<Value>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl FakeDaemon {
    fn start(path: &Path, responses: Value) -> Self {
        fs::create_dir_all(path.parent().expect("socket dir")).expect("socket dir");
        let listener = UnixListener::bind(path).expect("bind fake daemon socket");
        listener.set_nonblocking(true).expect("nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, frame_log) = (stop.clone(), frames.clone());
        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).ok();
                        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() {
                            continue;
                        }
                        let frame: Value = serde_json::from_str(&line)
                            .unwrap_or_else(|_| serde_json::json!({"_raw": line}));
                        let response = frame
                            .get("cmd")
                            .and_then(Value::as_str)
                            .and_then(|cmd| responses.get(cmd))
                            .or_else(|| responses.get("*"))
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({"success": true}));
                        frame_log.lock().expect("frames").push(frame);
                        let mut stream = stream;
                        let _ = writeln!(stream, "{response}");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(3));
                    }
                    Err(_) => return,
                }
            }
        });
        Self {
            stop,
            frames,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> Vec<Value> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.frames.lock().expect("frames").clone()
    }
}

fn session_of(argv: &[String]) -> String {
    argv.iter()
        .position(|a| a == "--session")
        .and_then(|i| argv.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "default".to_owned())
}

fn socket_path(home: &Path, session: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/symbrowse/run")
            .join(format!("{session}.sock"))
    } else {
        home.join("run/symbrowse").join(format!("{session}.sock"))
    }
}

fn short_root() -> tempfile::TempDir {
    let base = std::env::temp_dir();
    let builder = |dir: &Path| tempfile::Builder::new().prefix("c").tempdir_in(dir);
    if base.as_os_str().len() > 40 {
        builder(Path::new("/tmp")).expect("short temp root")
    } else {
        builder(&base).expect("temp root")
    }
}

struct Outcome {
    exit: i64,
    stdout: String,
    stderr: String,
    frames: Vec<Value>,
    files_after: BTreeMap<String, String>,
}

fn run_scenario(scenario: &Value) -> Outcome {
    let root = short_root();
    let rootp = root.path().canonicalize().expect("root");
    let home = rootp.join("h");
    let cwd = home.join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");
    if let Some(files) = scenario.get("files").and_then(Value::as_object) {
        let exec: Vec<&str> = scenario
            .get("exec_files")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for (rel, content) in files {
            let relative = rel.strip_prefix("home/").unwrap_or(rel);
            let target = home.join(relative);
            fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
            fs::write(&target, content.as_str().expect("file content")).expect("write");
            if exec.contains(&rel.as_str()) {
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).expect("chmod");
            }
        }
    }
    let argv: Vec<String> = scenario["argv"]
        .as_array()
        .expect("argv")
        .iter()
        .map(|v| v.as_str().expect("arg").to_owned())
        .collect();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrowse"));
    command
        .args(&argv)
        .current_dir(&cwd)
        .env_clear()
        .env("HOME", &home)
        .env(
            "PATH",
            scenario
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("/usr/bin:/bin"),
        )
        .env("TMPDIR", home.join("tmp"))
        .env("LANG", "C")
        .env("SYMBROWSE_NO_AUTOSTART", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !cfg!(target_os = "macos") {
        command.env("XDG_RUNTIME_DIR", home.join("run"));
    }
    fs::create_dir_all(home.join("tmp")).expect("tmp");
    if let Some(env) = scenario.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            command.env(
                key,
                value
                    .as_str()
                    .expect("env")
                    .replace("$HOME", &home.to_string_lossy()),
            );
        }
    }
    let daemon = (scenario["daemon"] == "up").then(|| {
        FakeDaemon::start(
            &socket_path(&home, &session_of(&argv)),
            scenario["responses"].clone(),
        )
    });
    let mut child = command.spawn().expect("spawn symbrowse");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let input = scenario.get("stdin").and_then(Value::as_str).unwrap_or("");
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    let frames = daemon.map(FakeDaemon::finish).unwrap_or_default();
    let kinds: Vec<String> = scenario
        .get("mask")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let norm = |bytes: &[u8]| -> String {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let text = text
            .replace(&rootp.to_string_lossy().into_owned(), "$ROOT")
            .replace(&root.path().to_string_lossy().into_owned(), "$ROOT");
        mask(&text, &kinds)
    };
    let mut files_after = BTreeMap::new();
    for rel in scenario
        .get("files_after")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Ok(bytes) = fs::read(cwd.join(rel)) {
            files_after.insert(rel.to_owned(), norm(&bytes));
        }
    }
    let frames_text = norm(&serde_json::to_vec(&frames).expect("frames"));
    Outcome {
        exit: i64::from(output.status.code().unwrap_or(-1)),
        stdout: norm(&output.stdout),
        stderr: norm(&output.stderr),
        frames: serde_json::from_str::<Vec<Value>>(&frames_text).expect("frames json"),
        files_after,
    }
}

/// Go marshals integral float64 values as `2`, serde_json as `2.0`; compare
/// numbers by value.
fn canonical_numbers(value: &Value) -> Value {
    match value {
        Value::Number(number) => number
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| value.clone(), Value::Number),
        Value::Array(items) => Value::Array(items.iter().map(canonical_numbers).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), canonical_numbers(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `compat-sidecar` is retired on purpose (browser-transport-contract.md): the
/// Go help and completion listings mention it, the Rust ones must not.
fn strip_retired(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| !line.trim_start().starts_with("compat-sidecar"))
        .collect()
}

/// The oracle was recorded on macOS, so untagged scenarios that report the
/// daemon socket carry the macOS default. Elsewhere the CLI uses
/// `$XDG_RUNTIME_DIR/symbrowse`, which `run_scenario` points at `$ROOT/h/run`
/// (see `socket_path`).
fn localize_socket_dir(text: &str) -> String {
    if cfg!(target_os = "macos") {
        return text.to_owned();
    }
    text.replace(
        "$ROOT/h/Library/Caches/symbrowse/run/",
        "$ROOT/h/run/symbrowse/",
    )
}

/// Untagged scenarios whose recorded outcome depends on a macOS-only facility.
/// `state-key-init-env` expects the Keychain lookup through `security` to
/// fail; other platforms have no Keychain step and report the environment key.
const UNTAGGED_DARWIN_ONLY: &[&str] = &["state-key-init-env"];

fn platform_mismatch(scenario: &Value) -> bool {
    let darwin_only = scenario
        .get("platform")
        .and_then(Value::as_str)
        .is_some_and(|p| p == "darwin")
        || scenario
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| UNTAGGED_DARWIN_ONLY.contains(&id));
    darwin_only && !cfg!(target_os = "macos")
}

#[test]
fn rust_cli_matches_frozen_go_cli_observations() {
    let oracle: Value = serde_json::from_str(ORACLE).expect("oracle json");
    let scenarios = oracle["scenarios"].as_array().expect("scenarios");
    assert!(
        scenarios.len() > 500,
        "oracle corpus shrank: {}",
        scenarios.len()
    );
    let only = std::env::var("CLI_ORACLE_ONLY").ok();
    let mut report = Vec::new();
    let mut json_report = serde_json::Map::new();
    let mut compared = 0usize;
    for scenario in scenarios {
        let id = scenario["id"].as_str().expect("id");
        if only.as_deref().is_some_and(|needle| !id.contains(needle)) || platform_mismatch(scenario)
        {
            continue;
        }
        compared += 1;
        let got = run_scenario(scenario);
        let want = &scenario["expected"];
        let want_stdout =
            localize_socket_dir(&strip_retired(want["stdout"].as_str().unwrap_or_default()));
        let want_stderr = want["stderr"].as_str().map(localize_socket_dir);
        let mut diffs = Vec::new();
        if want["exit"].as_i64() != Some(got.exit) {
            diffs.push(format!("exit: want {} got {}", want["exit"], got.exit));
        }
        if want_stdout != got.stdout {
            diffs.push(format!(
                "stdout:\n  want {want_stdout:?}\n  got  {:?}",
                got.stdout
            ));
        }
        if want_stderr.as_deref() != Some(got.stderr.as_str()) {
            diffs.push(format!(
                "stderr:\n  want {want_stderr:?}\n  got  {:?}",
                got.stderr
            ));
        }
        let want_frames = canonical_numbers(&want["frames"])
            .as_array()
            .cloned()
            .unwrap_or_default();
        let got_frames = canonical_numbers(&Value::Array(got.frames.clone()))
            .as_array()
            .cloned()
            .unwrap_or_default();
        if want_frames != got_frames {
            diffs.push(format!(
                "frames:\n  want {}\n  got  {}",
                Value::Array(want_frames),
                Value::Array(got.frames.clone())
            ));
        }
        let want_files: BTreeMap<String, String> = want
            .get("files_after")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        if want_files != got.files_after {
            diffs.push(format!(
                "files_after:\n  want {want_files:?}\n  got  {:?}",
                got.files_after
            ));
        }
        if !diffs.is_empty() {
            json_report.insert(
                id.to_owned(),
                serde_json::json!({
                    "argv": scenario["argv"],
                    "want": {"exit": want["exit"], "stdout": want_stdout, "stderr": want["stderr"], "frames": want["frames"]},
                    "got": {"exit": got.exit, "stdout": got.stdout, "stderr": got.stderr, "frames": got.frames},
                }),
            );
            report.push(format!(
                "## {id} {}\n{}",
                scenario["argv"],
                diffs.join("\n")
            ));
        }
    }
    if let Ok(path) = std::env::var("CLI_ORACLE_REPORT") {
        fs::write(
            path,
            serde_json::to_string_pretty(&json_report).expect("report"),
        )
        .expect("write report");
    }
    assert!(compared > 0, "no scenario selected");
    assert!(
        report.is_empty(),
        "{} of {compared} scenarios differ from the Go oracle (first 40 shown):\n{}",
        report.len(),
        report
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}
