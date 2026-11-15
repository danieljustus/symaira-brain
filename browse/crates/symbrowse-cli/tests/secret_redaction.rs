//! SEC-007: one synthetic credential corpus driven through the real `symbrowse`
//! binary (CLI, autostarted daemon and `mcp` stdio server). After the run, no
//! secret byte sequence may appear in any captured stdout/stderr or in any file
//! the processes wrote (journal, daemon log, cache, state, sockets' directory),
//! while the error classes stay useful (exit codes, codes, positions, hosts).
//!
//! Scope: failing/diagnostic output and everything written to disk. A
//! successful fetch result echoing the URL the caller itself supplied is data,
//! not a diagnostic, and is not part of this corpus (no successful fetch is
//! driven here; the targets refuse connections).

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

/// Synthetic secrets only. Each value is unique so a hit names its vector.
const URL_PASSWORD: &str = "SYNTH-SEC007-URLPW";
const URL_QUERY_TOKEN: &str = "SYNTH-SEC007-QTOK";
const URL_QUERY_APIKEY: &str = "SYNTH-SEC007-QKEY";
const ARGV_ASSIGNMENT: &str = "SYNTH-SEC007-ARGKV";
const ARGV_URL_PASSWORD: &str = "SYNTH-SEC007-ARGURL";
const CDP_ENV_PASSWORD: &str = "SYNTH-SEC007-CDPENV";
const CDP_TOML_PASSWORD: &str = "SYNTH-SEC007-CDPTOML";
const TOML_SYNTAX_PASSWORD: &str = "SYNTH-SEC007-TOMLPW";
const TOML_ADJACENT_TOKEN: &str = "SYNTH-SEC007-TOMLTOK";
const BAD_ENCRYPTION_KEY: &str = "SYNTH-SEC007-ENCBAD";
const PROXY_PASSWORD: &str = "SYNTH-SEC007-PROXY";
const MCP_METHOD_SECRET: &str = "SYNTH-SEC007-MCPM";
const MCP_TOOL_SECRET: &str = "SYNTH-SEC007-MCPT";
const MCP_ARGUMENT_SECRET: &str = "SYNTH-SEC007-MCPA";
/// A well-formed state encryption key (64 hex digits). It is a credential too.
const VALID_ENCRYPTION_KEY: &str =
    "5ec0075ec0075ec0075ec0075ec0075ec0075ec0075ec0075ec0075ec0075ec0";

const CORPUS: &[(&str, &str)] = &[
    ("url password", URL_PASSWORD),
    ("url query token", URL_QUERY_TOKEN),
    ("url query api_key", URL_QUERY_APIKEY),
    ("argv assignment", ARGV_ASSIGNMENT),
    ("argv url password", ARGV_URL_PASSWORD),
    ("cdp endpoint env", CDP_ENV_PASSWORD),
    ("cdp endpoint toml", CDP_TOML_PASSWORD),
    ("toml syntax line", TOML_SYNTAX_PASSWORD),
    ("toml adjacent line", TOML_ADJACENT_TOKEN),
    ("bad encryption key", BAD_ENCRYPTION_KEY),
    ("proxy password", PROXY_PASSWORD),
    ("mcp method", MCP_METHOD_SECRET),
    ("mcp tool", MCP_TOOL_SECRET),
    ("mcp argument", MCP_ARGUMENT_SECRET),
    ("valid encryption key", VALID_ENCRYPTION_KEY),
];

struct Root {
    path: PathBuf,
}

impl Root {
    fn new(name: &str) -> Self {
        // Short path: macOS AF_UNIX paths are limited to 104 bytes.
        let path = std::env::temp_dir().join(format!(
            "sx{name}{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        for dir in ["h", "b", "r", "c", "k", "d", "s"] {
            fs::create_dir_all(path.join(dir)).unwrap();
        }
        // A missing keychain tool is a failure, not an absent key (see
        // mcp_frozen_go_frames.rs); the environment key then wins.
        let security = path.join("b/security");
        fs::write(&security, "#!/bin/sh\nexit 44\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&security, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self { path }
    }

    fn command(&self, extra: &[(&str, &str)]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrowse"));
        command
            .current_dir(&self.path)
            .env_clear()
            .env("HOME", self.path.join("h"))
            .env("XDG_CONFIG_HOME", self.path.join("c"))
            .env("XDG_CACHE_HOME", self.path.join("k"))
            .env("XDG_DATA_HOME", self.path.join("d"))
            .env("XDG_STATE_HOME", self.path.join("s"))
            .env("XDG_RUNTIME_DIR", self.path.join("r"))
            .env("PATH", self.path.join("b"))
            .env("SYMBROWSE_ENGINE", "static")
            .env("SYMBROWSE_MODE", "static")
            .env("SYMBROWSE_ALLOW_PRIVATE", "1")
            .env("SYMBROWSE_SYMGUARD", "off")
            .env("SYMBROWSE_OOB_NOTIFY", "off")
            .env("SYMBROWSE_ENCRYPTION_KEY", VALID_ENCRYPTION_KEY)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            command.env(key, value);
        }
        command
    }

    fn config_file(&self, text: &str) {
        let directory = self.path.join("c/symbrowse");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("config.toml"), text).unwrap();
    }

    fn remove_config_file(&self) {
        let _ = fs::remove_file(self.path.join("c/symbrowse/config.toml"));
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
struct Run {
    label: String,
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Run {
    fn text(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

fn collect(mut command: Command, label: &str, stdin: Option<&[u8]>) -> Run {
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("spawn symbrowse");
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "{label}: timed out");
        std::thread::sleep(Duration::from_millis(20));
    };
    Run {
        label: label.to_owned(),
        code: status.code().unwrap_or(-1),
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

fn cli(root: &Root, label: &str, args: &[&str], extra: &[(&str, &str)]) -> Run {
    let mut command = root.command(extra);
    command.args(args);
    collect(command, label, None)
}

fn files_under(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            files_under(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
}

/// Every hit as `(where, vector)`; empty means the corpus never leaked.
fn leaks(runs: &[Run], root: &Root) -> Vec<(String, &'static str)> {
    let mut hits = Vec::new();
    let contains = |haystack: &[u8], needle: &str| {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    };
    for run in runs {
        for (stream, bytes) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            for (name, secret) in CORPUS {
                if contains(bytes, secret) {
                    hits.push((format!("{} {stream}", run.label), *name));
                }
            }
        }
    }
    let mut files = Vec::new();
    files_under(&root.path, &mut files);
    for file in files {
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        for (name, secret) in CORPUS {
            if contains(&bytes, secret) {
                hits.push((format!("file {}", file.display()), *name));
            }
        }
    }
    hits
}

fn corpus_url() -> String {
    format!(
        "http://reader:{URL_PASSWORD}@127.0.0.1:1/private/page?token={URL_QUERY_TOKEN}&x=1&api_key={URL_QUERY_APIKEY}"
    )
}

fn jsonrpc(messages: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for message in messages {
        bytes.extend_from_slice(message.to_string().as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": name, "arguments": arguments}})
}

/// Responses of an MCP run by request id.
fn responses(run: &Run) -> std::collections::BTreeMap<u64, Value> {
    String::from_utf8_lossy(&run.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| Some((value["id"].as_u64()?, value)))
        .collect()
}

#[test]
fn credential_corpus_never_reaches_output_or_files_and_errors_stay_useful() {
    let root = Root::new("a");
    let url = corpus_url();
    let mut runs: Vec<Run> = Vec::new();

    // --- CLI: argv echoed by the command parser --------------------------------
    let assignment = format!("password={ARGV_ASSIGNMENT}");
    let url_argument = format!("https://svc:{ARGV_URL_PASSWORD}@example.invalid/x");
    let unknown_assignment = cli(&root, "argv assignment", &[&assignment], &[]);
    assert_eq!(unknown_assignment.code, 2, "{}", unknown_assignment.text());
    assert_eq!(
        String::from_utf8_lossy(&unknown_assignment.stderr),
        "unknown command \"password=[REDACTED]\" for \"symbrowse\"\n",
        "error class and shape must survive redaction"
    );
    let unknown_url = cli(&root, "argv url", &[&url_argument], &[]);
    assert_eq!(unknown_url.code, 2);
    assert!(
        String::from_utf8_lossy(&unknown_url.stderr)
            .starts_with("unknown command \"https://svc:[REDACTED]@example.invalid/x\""),
        "{}",
        unknown_url.text()
    );
    runs.extend([unknown_assignment, unknown_url]);

    // --- CLI -> autostarted daemon (journal, daemon log, error responses) ------
    let mut proxy = String::from("http://proxy:");
    proxy.push_str(PROXY_PASSWORD);
    proxy.push_str("@127.0.0.1:1");
    for (label, command) in [("read", "read"), ("open", "open"), ("goto", "goto")] {
        let run = cli(
            &root,
            label,
            &[command, &url, "--session", "sec"],
            &[
                ("HTTPS_PROXY", &proxy),
                ("HTTP_PROXY", &proxy),
                ("ALL_PROXY", &proxy),
            ],
        );
        assert_eq!(run.code, 1, "{label}: {}", run.text());
        assert!(
            run.text().contains("request failed") && run.text().contains("127.0.0.1:1"),
            "{label}: error class and host must stay visible: {}",
            run.text()
        );
        runs.push(run);
    }
    let json_run = cli(
        &root,
        "read --json",
        &["read", &url, "--session", "sec", "--json"],
        &[],
    );
    let envelope: Value = serde_json::from_slice(&json_run.stdout).unwrap();
    assert_eq!(envelope["success"], false, "{}", json_run.text());
    assert!(envelope["error"]["code"].is_string());
    runs.push(json_run);
    let journal = cli(
        &root,
        "journal tail",
        &["journal", "tail", "--session", "sec", "--json"],
        &[],
    );
    assert_eq!(journal.code, 0, "{}", journal.text());
    assert!(
        journal.text().contains("read") && journal.text().contains("127.0.0.1:1/private/page"),
        "journal keeps command and URL path: {}",
        journal.text()
    );
    runs.push(journal);

    // --- MCP: tool calls, malformed method/tool names, batch items -------------
    let mcp_input = jsonrpc(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "sec007", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        tool_call(10, "open", json!({"url": url})),
        tool_call(11, "read", json!({"url": url})),
        tool_call(12, "fetch_url", json!({"url": url})),
        tool_call(13, "fetch_batch", json!({"urls": [url]})),
        tool_call(
            14,
            "fill",
            json!({"selector": "input", "value": format!("password={MCP_ARGUMENT_SECRET}")}),
        ),
        tool_call(15, &format!("token={MCP_TOOL_SECRET}"), json!({})),
        json!({"jsonrpc": "2.0", "id": 16, "method": format!("password={MCP_METHOD_SECRET}"),
               "params": {}}),
    ]);
    let mut command = root.command(&[("HTTPS_PROXY", &proxy)]);
    command.args([
        "mcp",
        "--tools",
        "all",
        "--allow-private",
        "--session",
        "mcp",
    ]);
    let mcp = collect(command, "mcp", Some(&mcp_input));
    assert_eq!(mcp.code, 0, "{}", mcp.text());
    let by_id = responses(&mcp);
    for id in [10, 11, 12] {
        let result = &by_id[&id]["result"];
        let tool_error = &result["_meta"]["symaira.dev/tool_error"];
        assert_eq!(
            tool_error["code"], "operation_failed",
            "id {id}: {}",
            by_id[&id]
        );
        assert!(
            tool_error["message"]
                .as_str()
                .is_some_and(|message| message.contains("request failed")),
            "id {id}: {}",
            by_id[&id]
        );
    }
    let batch = by_id[&13]["result"]["content"][0]["text"].as_str().unwrap();
    let batch: Value = serde_json::from_str(batch).unwrap();
    assert_eq!(batch[0]["ok"], false);
    assert_eq!(batch[0]["code"], "operation_failed");
    assert!(
        batch[0]["url"]
            .as_str()
            .is_some_and(|value| value.contains("127.0.0.1:1/private/page")),
        "failed batch item keeps its target: {batch}"
    );
    assert_eq!(by_id[&15]["error"]["code"], -32601);
    assert!(
        by_id[&15]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("Unknown tool: token=")),
        "{}",
        by_id[&15]
    );
    assert_eq!(by_id[&16]["error"]["code"], -32601);
    assert!(
        by_id[&16]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("Method not found: password=")),
        "{}",
        by_id[&16]
    );
    runs.push(mcp);

    // --- configuration: environment, TOML (syntax + type errors), bad key ------
    let cdp_env = format!("http://dev:{CDP_ENV_PASSWORD}@127.0.0.1:9");
    let shown = cli(
        &root,
        "config show cdp env",
        &["config", "show"],
        &[("SYMBROWSE_CDP_ENDPOINT", &cdp_env)],
    );
    assert_eq!(shown.code, 0, "{}", shown.text());
    assert!(
        shown
            .text()
            .contains("cdp_endpoint=http://dev:[REDACTED]@127.0.0.1:9 (source: env)"),
        "{}",
        shown.text()
    );
    runs.push(shown);

    root.config_file(&format!(
        "cdp_endpoint = \"http://dev:{CDP_TOML_PASSWORD}@127.0.0.1:9\"\n"
    ));
    let shown = cli(&root, "config show cdp toml", &["config", "show"], &[]);
    assert_eq!(shown.code, 0, "{}", shown.text());
    assert!(
        shown
            .text()
            .contains("cdp_endpoint=http://dev:[REDACTED]@127.0.0.1:9 (source: global)")
    );
    runs.push(shown);

    root.config_file(&format!(
        "api_token = \"{TOML_ADJACENT_TOKEN}\"\npassword = \"{TOML_SYNTAX_PASSWORD}\" oops\n"
    ));
    for (label, args) in [
        ("config show broken toml", vec!["config", "show"]),
        (
            "daemon broken toml",
            vec!["daemon", "--session", "brokentoml"],
        ),
    ] {
        let run = cli(&root, label, &args, &[]);
        assert!(run.code != 0, "{label}: {}", run.text());
        assert!(
            run.text().contains(
                "failed to load global configuration: TOML parse error at line 2, column "
            ),
            "{label}: position and class must stay: {}",
            run.text()
        );
        runs.push(run);
    }
    root.config_file(&format!("read_timeout = \"{TOML_SYNTAX_PASSWORD}\"\n"));
    let typed = cli(&root, "config show typed toml", &["config", "show"], &[]);
    assert!(typed.text().contains("TOML parse error at line 1, column "));
    assert!(typed.text().contains("invalid type"), "{}", typed.text());
    runs.push(typed);
    root.remove_config_file();

    let bad_key = cli(
        &root,
        "daemon bad key",
        &["daemon", "--session", "badkey"],
        &[("SYMBROWSE_ENCRYPTION_KEY", BAD_ENCRYPTION_KEY)],
    );
    assert_eq!(bad_key.code, 1, "{}", bad_key.text());
    assert!(
        bad_key
            .text()
            .contains("key must be a 64-character hex string"),
        "{}",
        bad_key.text()
    );
    runs.push(bad_key);

    // --- shutdown, then scan every output and every file -----------------------
    for session in ["sec", "mcp"] {
        let stop = cli(
            &root,
            &format!("daemon stop {session}"),
            &["daemon", "stop", "--session", session],
            &[],
        );
        runs.push(stop);
    }
    // The daemon log must exist, otherwise the scan proves nothing about it.
    let mut files = Vec::new();
    files_under(&root.path, &mut files);
    assert!(
        files.iter().any(|file| file.ends_with("daemon.log")),
        "daemon log was not written: {files:?}"
    );
    assert!(
        files
            .iter()
            .any(|file| file.to_string_lossy().contains("journal")),
        "journal was not written: {files:?}"
    );

    let hits = leaks(&runs, &root);
    assert!(hits.is_empty(), "credential bytes leaked: {hits:#?}");
}

#[test]
fn scanner_detects_every_corpus_secret_in_output_and_files() {
    // The assertion above is only meaningful if the scanner can see each
    // secret, in stdout, in stderr and in a written file.
    let root = Root::new("b");
    let planted: Vec<Run> = CORPUS
        .iter()
        .enumerate()
        .map(|(index, (_, secret))| Run {
            label: format!("plant {index}"),
            code: 0,
            stdout: if index % 2 == 0 {
                format!("noise {secret} noise").into_bytes()
            } else {
                Vec::new()
            },
            stderr: if index % 2 == 1 {
                format!("noise {secret} noise").into_bytes()
            } else {
                Vec::new()
            },
        })
        .collect();
    assert_eq!(leaks(&planted, &root).len(), CORPUS.len());
    fs::write(root.path.join("s/journal.jsonl"), CORPUS[3].1).unwrap();
    let clean = [Run {
        label: "clean".into(),
        code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
    }];
    let hits = leaks(&clean, &root);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, CORPUS[3].0);
}
