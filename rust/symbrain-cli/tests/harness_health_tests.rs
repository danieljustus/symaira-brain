#![cfg(unix)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::json;
use tempfile::TempDir;

const HEALTHY_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import sys

for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
    elif request.get("id") is not None and request.get("method") == "ping":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {}}), flush=True)
"#;

const BARRIER_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import os
import sys
import time

ready_dir = sys.argv[1]
for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        with open(os.path.join(ready_dir, str(os.getpid())), "x"):
            pass
        while len(os.listdir(ready_dir)) < 2:
            time.sleep(0.01)
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
    elif request.get("id") is not None and request.get("method") == "ping":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {}}), flush=True)
"#;

const INVALID_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import sys

for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "wrong-version",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
"#;

const PERSISTENT_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import os
import sys
import time

with open(sys.argv[1], "w") as pid:
    pid.write(str(os.getpid()))
for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
    elif request.get("id") is not None and request.get("method") == "ping":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {}}), flush=True)
        time.sleep(30)
"#;

const ERROR_PING_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import sys

for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
    elif request.get("id") is not None and request.get("method") == "ping":
        print("stdio-stderr-canary", file=sys.stderr, flush=True)
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {
            "code": -1, "message": "stdio-body-canary"
        }}), flush=True)
"#;

const TIMEOUT_PING_MCP: &[u8] = br#"#!/usr/bin/python3
import json
import sys
import time

for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") is not None and request.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "fake", "version": "1"}
        }}), flush=True)
    elif request.get("id") is not None and request.get("method") == "ping":
        time.sleep(30)
"#;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    let cache = root.join("cache");
    let project = root.join("project");
    for path in [&home, &config, &data, &cache, &project] {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .args(args);
    command
}

fn executable(root: &TempDir, name: &str, contents: &[u8]) -> std::path::PathBuf {
    let path = root.path().join(name);
    std::fs::write(&path, contents).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

fn write_servers(root: &TempDir, servers: &serde_json::Value) {
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    std::fs::write(
        root.path().join("home/.claude.json"),
        serde_json::to_vec(&json!({"mcpServers": servers})).unwrap(),
    )
    .unwrap();
}

fn run(root: &TempDir, args: &[&str]) -> Output {
    command(root, args).output().unwrap()
}

#[test]
fn native_health_reports_unsupported_and_probes_stdio() {
    let root = TempDir::new().unwrap();
    let fake = executable(&root, "healthy-mcp.py", HEALTHY_MCP);
    write_servers(
        &root,
        &json!({
            "zeta": {"command": fake},
            "alpha": {"transport": "unsupported"},
            "empty": {"command": ""}
        }),
    );

    let output = run(&root, &["harness", "health", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert!(!output.stdout[..output.stdout.len() - 1].contains(&b'\n'));

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["health_schema_version"], 1);
    let servers = report["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 3);
    assert_eq!(servers[0]["server"], "alpha");
    assert_eq!(servers[0]["outcome"], "unsupported");
    assert_eq!(
        servers[0]["error"],
        "configured transport is not supported for health probing"
    );
    assert!(servers[0].get("probe_method").is_none());
    assert!(servers[0].get("latency_ms").is_none());
    assert_eq!(servers[1]["server"], "empty");
    assert_eq!(servers[1]["outcome"], "unsupported");
    assert_eq!(
        servers[1]["error"],
        "stdio server has no configured command"
    );
    assert_eq!(servers[2]["server"], "zeta");
    assert_eq!(servers[2]["healthy"], true);
    assert_eq!(servers[2]["outcome"], "healthy");
    assert_eq!(servers[2]["probe_method"], "initialize+ping");
    assert!(
        servers[2]["latency_ms"]
            .as_f64()
            .is_some_and(|ms| ms >= 0.0)
    );
    assert!(servers[2].get("error").is_none());

    let table = run(&root, &["harness", "health"]);
    assert!(table.status.success(), "stderr: {:?}", table.stderr);
    let table = String::from_utf8(table.stdout).unwrap();
    assert!(table.contains("  ✗  claude       alpha          "));
    assert!(table.contains("  ✗  claude       empty          "));
    assert!(table.contains("  ✓  claude       zeta           "));
    assert!(table.find("alpha").unwrap() < table.find("empty").unwrap());
    assert!(table.find("empty").unwrap() < table.find("zeta").unwrap());
}

#[test]
fn multiple_native_probes_run_in_parallel_and_are_sorted() {
    let root = TempDir::new().unwrap();
    let ready_dir = root.path().join("ready");
    std::fs::create_dir(&ready_dir).unwrap();
    let fake = executable(&root, "barrier-mcp.py", BARRIER_MCP);
    write_servers(
        &root,
        &json!({
            "zeta": {"command": fake, "args": [ready_dir]},
            "alpha": {"command": fake, "args": [ready_dir]}
        }),
    );

    let output = run(&root, &["harness", "health", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let servers = report["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0]["server"], "alpha");
    assert_eq!(servers[1]["server"], "zeta");
    assert!(servers.iter().all(|server| server["healthy"] == true));
}

#[test]
fn empty_health_matches_go_shapes() {
    let root = TempDir::new().unwrap();

    let table = run(&root, &["harness", "health"]);
    assert!(table.status.success(), "stderr: {:?}", table.stderr);
    assert_eq!(table.stdout, b"no MCP servers found\n");

    let json = run(&root, &["harness", "health", "--json"]);
    assert!(json.status.success(), "stderr: {:?}", json.stderr);
    assert_eq!(json.stdout, b"{\"servers\":null}\n");
}

#[test]
fn protocol_mismatch_is_redacted_without_fallback() {
    let root = TempDir::new().unwrap();
    let fake = executable(&root, "invalid-mcp.py", INVALID_MCP);
    write_servers(&root, &json!({"broken": {"command": fake}}));
    let fallback = executable(
        &root,
        "go-fallback",
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 29\n",
    );

    let mut command = command(&root, &["harness", "health", "--json"]);
    let output = command
        .env("SYMBRAIN_GO_BINARY", fallback)
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["health_schema_version"], 1);
    assert_eq!(report["servers"][0]["healthy"], false);
    assert_eq!(report["servers"][0]["outcome"], "unhealthy");
    assert_eq!(report["servers"][0]["probe_method"], "initialize");
    assert!(report["servers"][0]["latency_ms"].as_f64().is_some());
    assert_eq!(report["servers"][0]["error"], "MCP initialize failed");
}

#[test]
fn multiple_missing_stdio_probes_are_reported_natively() {
    let root = TempDir::new().unwrap();
    write_servers(
        &root,
        &json!({
            "first": {"command": "missing-first"},
            "second": {"command": "missing-second"}
        }),
    );
    let mut command = command(&root, &["harness", "health", "--json"]);
    let output = command
        .env(
            "SYMBRAIN_GO_BINARY",
            root.path().join("missing-go-fallback"),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let servers = report["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0]["server"], "first");
    assert_eq!(servers[1]["server"], "second");
    assert!(servers.iter().all(|server| server["healthy"] == false));
    assert!(
        servers
            .iter()
            .all(|server| server["outcome"] == "unhealthy")
    );
    assert!(
        servers
            .iter()
            .all(|server| server.get("probe_method").is_none())
    );
    assert!(
        servers
            .iter()
            .all(|server| server.get("latency_ms").is_none())
    );
}

#[test]
fn stdio_ping_error_is_redacted_and_silent_ping_hits_the_deadline() {
    let root = TempDir::new().unwrap();
    let error_child = executable(&root, "error-ping.py", ERROR_PING_MCP);
    let timeout_child = executable(&root, "timeout-ping.py", TIMEOUT_PING_MCP);
    write_servers(
        &root,
        &json!({
            "error": {"command": error_child},
            "timeout": {"command": timeout_child}
        }),
    );

    let started = Instant::now();
    let output = run(&root, &["harness", "health", "--json"]);
    let elapsed = started.elapsed();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    assert!(
        elapsed < Duration::from_secs(7),
        "health elapsed {elapsed:?}"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("stdio-stderr-canary"));
    assert!(!text.contains("stdio-body-canary"));

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let servers = report["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0]["server"], "error");
    assert_eq!(servers[0]["outcome"], "unhealthy");
    assert_eq!(servers[0]["probe_method"], "initialize+ping");
    assert!(servers[0]["latency_ms"].as_f64().is_some());
    assert_eq!(servers[0]["error"], "MCP ping failed");
    assert_eq!(servers[1]["server"], "timeout");
    assert_eq!(servers[1]["outcome"], "unhealthy");
    assert_eq!(servers[1]["probe_method"], "initialize+ping");
    assert!(servers[1]["latency_ms"].as_f64().is_some());
    assert_eq!(servers[1]["error"], "MCP probe timed out");
}

#[test]
fn successful_probe_reaps_persistent_child() {
    let root = TempDir::new().unwrap();
    let pid_path = root.path().join("child.pid");
    let fake = executable(&root, "persistent-mcp.py", PERSISTENT_MCP);
    write_servers(
        &root,
        &json!({"persistent": {"command": fake, "args": [pid_path]}}),
    );

    let output = run(&root, &["harness", "health", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let pid = std::fs::read_to_string(&pid_path).unwrap();
    let pid = pid.trim();
    for _ in 0..20 {
        let status = Command::new("/bin/kill")
            .args(["-0", pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if !status.success() {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    panic!("persistent MCP child {pid} was not reaped");
}
