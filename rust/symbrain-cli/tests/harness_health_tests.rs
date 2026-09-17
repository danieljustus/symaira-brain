#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::Duration;

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
fn native_health_probes_stdio_and_skips_other_transports() {
    let root = TempDir::new().unwrap();
    let fake = executable(&root, "healthy-mcp.py", HEALTHY_MCP);
    write_servers(
        &root,
        &json!({
            "zeta": {"command": fake},
            "alpha": {"url": "https://example.test/mcp"},
            "empty": {"command": ""}
        }),
    );

    let output = run(&root, &["harness", "health", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert!(!output.stdout[..output.stdout.len() - 1].contains(&b'\n'));

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let servers = report["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 3);
    assert_eq!(servers[0]["server"], "alpha");
    assert_eq!(
        servers[0]["error"],
        "not probed: http transport is not stdio"
    );
    assert_eq!(servers[1]["server"], "empty");
    assert_eq!(
        servers[1]["error"],
        "not probed: stdio transport is not stdio"
    );
    assert_eq!(servers[2]["server"], "zeta");
    assert_eq!(servers[2]["healthy"], true);
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
fn probe_failure_falls_back_without_native_output() {
    let root = TempDir::new().unwrap();
    write_servers(&root, &json!({"broken": {"command": "missing-mcp"}}));
    let fallback = executable(
        &root,
        "go-fallback",
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 17\n",
    );

    let mut command = command(&root, &["harness", "health", "--json"]);
    let output = command
        .env("SYMBRAIN_GO_BINARY", fallback)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");
}

#[test]
fn protocol_failure_falls_back_without_native_output() {
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
    assert_eq!(output.status.code(), Some(29));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");
}

#[test]
fn multiple_stdio_probes_fall_back_without_native_output() {
    let root = TempDir::new().unwrap();
    write_servers(
        &root,
        &json!({
            "first": {"command": "missing-first"},
            "second": {"command": "missing-second"}
        }),
    );
    let fallback = executable(
        &root,
        "go-fallback",
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 23\n",
    );

    let mut command = command(&root, &["harness", "health", "--json"]);
    let output = command
        .env("SYMBRAIN_GO_BINARY", fallback)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");
}

#[test]
fn malformed_config_falls_back_without_native_output() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    std::fs::write(root.path().join("home/.claude.json"), b"{not-json").unwrap();
    let fallback = executable(
        &root,
        "go-fallback",
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 19\n",
    );

    let mut command = command(&root, &["harness", "health", "--json"]);
    let output = command
        .env("SYMBRAIN_GO_BINARY", fallback)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(19));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");
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
