//! Independent subprocess coverage for the native MCP CLI cutover.

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const FAKE_MCP: &str = r#"#!/usr/bin/python3
import json
import os
import sys
import time

pid_file = os.environ.get("FAKE_MCP_PID_FILE")
if pid_file:
    with open(pid_file, "w", encoding="ascii") as f:
        f.write(str(os.getpid()))

def send(request_id, result=None, error=None):
    response = {"jsonrpc": "2.0", "id": request_id}
    if error is not None:
        response["error"] = error
    else:
        response["result"] = result
    print(json.dumps(response, separators=(",", ":")), flush=True)

for line in sys.stdin:
    try:
        request = json.loads(line)
    except Exception:
        continue
    if "id" not in request:
        continue
    request_id = request["id"]
    method = request.get("method")
    if method == "initialize":
        send(request_id, {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}})
    elif method == "tools/list":
        send(request_id, {"tools": [{"name": "echo", "description": "echo", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": True}}]})
    elif method == "tools/call":
        params = request.get("params", {})
        send(request_id, {"content": [{"type": "text", "text": json.dumps(params.get("arguments", {}), separators=(",", ":"))}], "isError": False})
    else:
        send(request_id, error={"code": -32601, "message": "Method not found"})
"#;

fn write_fake(root: &TempDir) -> std::path::PathBuf {
    let path = root.path().join("fake-mcp.py");
    std::fs::write(&path, FAKE_MCP).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}

fn write_profile(root: &TempDir, fake: &std::path::Path) -> std::path::PathBuf {
    let path = root.path().join("room.toml");
    let command = fake.to_str().unwrap().replace('"', "\\\"");
    std::fs::write(
        &path,
        format!(
            "[profile]\nname = \"room\"\n\n[servers.foreign]\nenabled = true\ncommand = \"{command}\"\naccess = \"write\"\n"
        ),
    )
    .unwrap();
    path
}

fn command(root: &TempDir, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .args(args)
        .env("HOME", root.path().join("home"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("PATH", root.path().join("empty-path"))
        .env_remove("SYMBRAIN_GO_BINARY")
        .env_remove("SYMBRAIN_SERVERS_VAULT_BINARY_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run_with_input(root: &TempDir, args: &[&str], input: &[u8]) -> Output {
    let mut child = command(root, args).spawn().unwrap();
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
fn mcp_subprocess_runs_native_initialize_list_call_and_silent_notification() {
    let root = TempDir::new().unwrap();
    let fake = write_fake(&root);
    let profile = write_profile(&root, &fake);
    let profile = profile.to_str().unwrap();
    let input = concat!(
        "{bad}\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"x":1}}}"#,
        "\n",
    );
    let output = run_with_input(&root, &["mcp", "--profile-file", profile], input.as_bytes());
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );

    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        responses.len(),
        4,
        "notification must not receive a response"
    );
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 1);
    assert_eq!(responses[1]["result"]["serverInfo"]["name"], "symbrain");
    assert_eq!(responses[2]["id"], 2);
    let tools = responses[2]["result"]["tools"].as_array().unwrap();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["bootstrap", "patterns", "echo"]);
    assert_eq!(responses[3]["id"], 3);
    assert_eq!(responses[3]["result"]["isError"], false);
    assert_eq!(responses[3]["result"]["content"][0]["text"], r#"{"x":1}"#);
}

#[test]
fn native_mcp_audit_creates_redacted_jsonl_without_stdout_pollution() {
    let root = TempDir::new().unwrap();
    let fake = write_fake(&root);
    let profile = root.path().join("audited.toml");
    std::fs::write(
        &profile,
        format!(
            "[profile]\nname = \"audited\"\n\n[audit]\nenabled = true\nverbose = true\n\n[servers.foreign]\nenabled = true\ncommand = \"{}\"\naccess = \"write\"\n",
            fake.display()
        ),
    )
    .unwrap();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"token":"token-value","content":"private-content","query":"visible"}}}"#,
        "\n"
    );
    let output = run_with_input(
        &root,
        &["mcp", "--profile-file", profile.to_str().unwrap()],
        input.as_bytes(),
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
    assert!(!output.stdout.is_empty());

    let audit_path = root
        .path()
        .join("home/.local/share/symbrain/audit/audited.jsonl");
    assert!(audit_path.is_file(), "audit log was not created");
    let audit = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit.contains("arg_keys"));
    assert!(audit.contains("[redacted]"));
    assert!(!audit.contains("token-value"));
    assert!(!audit.contains("private-content"));
    assert!(audit.contains("query=visible"));
}

#[test]
fn mcp_profile_flag_loads_xdg_profile_directory() {
    let root = TempDir::new().unwrap();
    let fake = write_fake(&root);
    let profiles = root.path().join("config").join("symbrain").join("profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    let profile = profiles.join("named.toml");
    let command = fake.to_str().unwrap();
    std::fs::write(
        &profile,
        format!(
            "[profile]\nname = \"named\"\n\n[servers.foreign]\nenabled = true\ncommand = \"{command}\"\naccess = \"write\"\n"
        ),
    )
    .unwrap();
    let output = run_with_input(&root, &["mcp", "--profile", "named"], b"");
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn deprecated_serve_alias_is_native_and_warns_only_on_stderr() {
    let root = TempDir::new().unwrap();
    let fake = write_fake(&root);
    let profile = write_profile(&root, &fake);
    let output = run_with_input(
        &root,
        &["serve", "--profile-file", profile.to_str().unwrap()],
        b"",
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(
        output.stdout.is_empty(),
        "stdout pollution: {:?}",
        output.stdout
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("'serve' is deprecated"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Go fallback"));
}

#[test]
fn mcp_profile_errors_and_unknown_flags_do_not_fallback() {
    let root = TempDir::new().unwrap();
    for args in [
        vec!["mcp"],
        vec!["mcp", "--profile", "one", "--profile-file", "two.toml"],
        vec!["mcp", "--bogus"],
        vec!["mcp", "--profile", "missing"],
    ] {
        let output = run_with_input(&root, &args, b"");
        assert_eq!(
            output.status.code(),
            Some(2),
            "args={args:?}, stderr={:?}",
            output.stderr
        );
        assert!(output.stdout.is_empty());
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("not ported yet and no Go fallback")
        );
    }
}

#[test]
fn enabled_unported_skills_server_fails_closed_without_placeholder_tools() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("blocked.toml");
    std::fs::write(
        &profile,
        "[profile]\nname = \"blocked\"\n\n[servers.skills]\nenabled = true\nmode = \"read_only\"\n",
    )
    .unwrap();
    let output = run_with_input(
        &root,
        &["mcp", "--profile-file", profile.to_str().unwrap()],
        b"",
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("native embedded handlers are not ported for skills"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("no Go fallback"));
}

#[cfg(unix)]
#[test]
fn sigterm_cancels_gateway_and_terminates_child_process_group() {
    let root = TempDir::new().unwrap();
    let fake = write_fake(&root);
    let profile = write_profile(&root, &fake);
    let pid_file = root.path().join("child.pid");
    let mut child = command(&root, &["mcp", "--profile-file", profile.to_str().unwrap()])
        .env("FAKE_MCP_PID_FILE", &pid_file)
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while !pid_file.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(pid_file.exists(), "child was not spawned");
    let child_deadline = Instant::now() + Duration::from_secs(5);
    let child_pid: i32 = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|contents| contents.trim().parse().ok())
        {
            break pid;
        }
        assert!(Instant::now() < child_deadline, "child pid was not written");
        thread::sleep(Duration::from_millis(10));
    };
    Command::new("/bin/kill")
        .args(["-TERM", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "native mcp did not stop after SIGTERM"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "status={status:?}");

    let child_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < child_deadline {
        let alive = Command::new("/bin/kill")
            .args(["-0", &child_pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let child_alive = Command::new("/bin/kill")
        .args(["-0", &child_pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(!child_alive, "child process survived shutdown");
    let output = child.wait_with_output().unwrap();
    assert!(output.stdout.is_empty());
}

#[test]
fn usage_subprocess_lists_and_calls_native_tool_without_go_fallback() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("usage.toml");
    std::fs::write(
        &profile,
        "[profile]\nname = \"usage\"\n\n[audit]\nenabled = true\nverbose = true\n\n[servers.usage]\nenabled = true\n",
    )
    .unwrap();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_ai_usage","arguments":{}}}"#,
        "\n"
    );
    let output = run_with_input(
        &root,
        &["mcp", "--profile-file", profile.to_str().unwrap()],
        input.as_bytes(),
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["tools"][2]["name"], "get_ai_usage");
    assert_eq!(responses[1]["result"]["isError"], false);
    let report: serde_json::Value = serde_json::from_str(
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .expect("usage report text"),
    )
    .expect("usage report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["providers"].as_array().unwrap().len(), 10);
    let audit_path = root
        .path()
        .join("home/.local/share/symbrain/audit/usage.jsonl");
    let audit = std::fs::read_to_string(audit_path).expect("usage audit");
    let records = audit
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        records.iter().any(|record| {
            record
                .get("d")
                .and_then(serde_json::Value::as_str)
                .and_then(|data| serde_json::from_str::<serde_json::Value>(data).ok())
                .is_some_and(|data| data["server"] == "usage" && data["tool"] == "get_ai_usage")
        }),
        "audit records: {records:?}"
    );
}
