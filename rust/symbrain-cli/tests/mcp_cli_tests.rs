//! Independent subprocess evidence for the native MCP CLI migration.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

#[cfg(unix)]
#[path = "support/mcp_lifecycle.rs"]
mod mcp_lifecycle;

const FAKE_MCP: &str = r#"#!/usr/bin/python3
import json
import sys

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

struct FakeCommand {
    command: std::path::PathBuf,
    args: Vec<String>,
}

fn write_fake(root: &TempDir) -> FakeCommand {
    #[cfg(windows)]
    {
        let path = root.path().join("fake-mcp.py");
        std::fs::write(&path, FAKE_MCP).unwrap();
        let output = Command::new("python")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .expect("Python is required by the native MCP fixture");
        assert!(output.status.success(), "Python lookup failed: {output:?}");
        let python = String::from_utf8(output.stdout).unwrap();
        FakeCommand {
            command: python.trim().into(),
            args: vec![path.to_string_lossy().into_owned()],
        }
    }
    #[cfg(not(windows))]
    {
        let path = root.path().join("fake-mcp.py");
        std::fs::write(&path, FAKE_MCP).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
        FakeCommand {
            command: path,
            args: Vec::new(),
        }
    }
}

fn toml_string(value: &str) -> String {
    toml_edit::Value::from(value).to_string()
}

fn toml_args(args: &[String]) -> String {
    let mut values = toml_edit::Array::new();
    for arg in args {
        values.push(arg.as_str());
    }
    values.to_string()
}

fn write_profile(root: &TempDir, fake: &FakeCommand) -> std::path::PathBuf {
    let path = root.path().join("room.toml");
    std::fs::write(
        &path,
        format!(
            "[profile]\nname = \"room\"\n\n[servers.foreign]\nenabled = true\ncommand = {}\nargs = {}\naccess = \"write\"\n",
            toml_string(&fake.command.to_string_lossy()),
            toml_args(&fake.args)
        ),
    )
    .unwrap();
    path
}

fn command(root: &TempDir, args: &[&str]) -> Command {
    if args.first() == Some(&"usage") {
        let claude_file = root.path().join("home/.claude/.credentials.json");
        match std::fs::symlink_metadata(&claude_file) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // `needs_go_fallback` checks Claude Keychain presence when
                // the default file is absent. Give usage-route subprocesses a
                // synthetic file so tests never enumerate the host Keychain.
                install_synthetic_claude_file(root);
            }
            Err(error) => panic!("inspect synthetic Claude credentials path: {error}"),
        }
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command.env_clear().envs(coverage::profile_environment());
    #[cfg(windows)]
    {
        for key in [
            "SystemRoot",
            "SYSTEMROOT",
            "windir",
            "WINDIR",
            "PATHEXT",
            "ComSpec",
            "COMSPEC",
            "TEMP",
            "TMP",
            "SystemDrive",
        ] {
            if let Some(val) = std::env::var_os(key) {
                command.env(key, val);
            }
        }
    }
    command
        .args(args)
        .env("HOME", root.path().join("home"))
        .env("USERPROFILE", root.path().join("home"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("home/.local/share"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_RUNTIME_DIR", root.path().join("runtime"))
        .env("PATH", root.path().join("empty-path"))
        .env("TMPDIR", root.path())
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn install_synthetic_claude_file(root: &TempDir) {
    let path = root.path().join("home/.claude/.credentials.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        br#"{"oauthAccount":{"default":{"accessToken":"synthetic-claude-file"}}}"#,
    )
    .unwrap();
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
    assert_eq!(
        names,
        ["bootstrap", "patterns", "echo"],
        "foreign server tools were not merged: {:?}",
        responses[2]
    );
    assert_eq!(responses[3]["id"], 3);
    assert_eq!(responses[3]["result"]["isError"], false);
    assert_eq!(responses[3]["result"]["content"][0]["text"], r#"{"x":1}"#);
}

#[test]
fn native_mcp_audit_creates_redacted_jsonl_without_stdout_pollution() {
    let root = TempDir::new().unwrap();
    // Frozen Go takes verbosity from resolved Brain config, even when the
    // profile enables audit. Exercise verbose redaction through that owner.
    let config = root.path().join("config/symbrain");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "audit.verbose=true\n").unwrap();
    let fake = write_fake(&root);
    let profile = root.path().join("audited.toml");
    std::fs::write(
        &profile,
        format!(
            "[profile]\nname = \"audited\"\n\n[audit]\nenabled = true\nverbose = true\n\n[servers.foreign]\nenabled = true\ncommand = {}\nargs = {}\naccess = \"write\"\n",
            toml_string(&fake.command.to_string_lossy()),
            toml_args(&fake.args)
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
    assert!(audit.contains("[redacted]"), "actual audit: {audit}");
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
    std::fs::write(
        &profile,
        format!(
            "[profile]\nname = \"named\"\n\n[servers.foreign]\nenabled = true\ncommand = {}\nargs = {}\naccess = \"write\"\n",
            toml_string(&fake.command.to_string_lossy()),
            toml_args(&fake.args)
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
fn enabled_skills_server_exposes_native_tools_without_go_fallback() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("skills.toml");
    std::fs::write(
        &profile,
        "[profile]\nname = \"skills\"\n\n[audit]\nenabled = true\nverbose = true\n\n[servers.skills]\nenabled = true\n",
    )
    .unwrap();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skills_list","arguments":{}}}"#,
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
    let tools = responses[0]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "skills_list"));
    assert_eq!(responses[1]["result"]["isError"], false);
}

#[cfg(unix)]
#[test]
fn sigterm_cancels_gateway_and_terminates_child_process_group() {
    mcp_lifecycle::assert_sigterm_shutdown();
}

#[cfg(unix)]
#[test]
fn sigterm_while_idle_exits_cleanly_with_stdin_open() {
    mcp_lifecycle::assert_idle_sigterm_shutdown();
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
    let mut child = command(&root, &["mcp", "--profile-file", profile.to_str().unwrap()])
        // Make provider discovery fail before its macOS Keychain fallback. The
        // test exercises the MCP route and report shape, not real credentials.
        .env(
            "ANTHROPIC_OAUTH_TOKEN",
            "env://SYMBRAIN_USAGE_TEST_MISSING_TOKEN",
        )
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
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
    assert!(
        report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|provider| provider["snapshot"].is_null())
    );
    for provider in report["providers"].as_array().unwrap() {
        if provider["id"] == "antigravity" {
            assert_eq!(provider["configured"], true);
            assert!(
                provider["error"]
                    .as_str()
                    .unwrap()
                    .contains("Antigravity is not running")
            );
        } else {
            assert_eq!(provider["configured"], false, "provider: {provider}");
        }
    }
    let claude = report["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["id"] == "claude")
        .expect("Claude provider");
    assert_eq!(claude["configured"], false);
    assert!(claude["snapshot"].is_null());
    assert!(
        claude["auth_status"]["detail"]
            .as_str()
            .unwrap()
            .contains("secret resolution failed")
    );
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

#[test]
fn direct_claude_oauth_routes_usage_to_native_parser_without_provider_request() {
    let root = TempDir::new().unwrap();
    // The invalid flag returns before any report fetch. With PATH empty and
    // provider variables cleared by `command`, this proves route selection
    // for a synthetic direct OAuth token without contacting Anthropic.
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .env("ANTHROPIC_OAUTH_TOKEN", "synthetic-direct-oauth-fixture")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
        "native usage parser did not handle the request: {stderr}"
    );
    assert!(
        !stderr.contains("no Go fallback") && !stderr.contains("Go fallback"),
        "direct Claude OAuth unexpectedly selected Go fallback: {stderr}"
    );
}

#[test]
fn claude_oauth_file_routes_usage_to_native_parser_without_keychain_or_provider_request() {
    let root = TempDir::new().unwrap();
    let credentials = root
        .path()
        .join("home")
        .join(".claude")
        .join(".credentials.json");
    std::fs::create_dir_all(credentials.parent().unwrap()).unwrap();
    std::fs::write(
        &credentials,
        br#"{"oauthAccount":{"default":{"accessToken":"synthetic-claude-file-fixture"}}}"#,
    )
    .unwrap();

    // The valid file token wins before Claude's Keychain fallback. The invalid
    // flag is handled before any report fetch, so this proves file-source route
    // selection without reading Keychain secrets or contacting Anthropic.
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
        "native usage parser did not handle the request: {stderr}"
    );
    assert!(
        !stderr.contains("no Go fallback") && !stderr.contains("Go fallback"),
        "Claude file OAuth unexpectedly selected Go fallback: {stderr}"
    );

    let mixed = command(&root, &["usage", "--not-a-usage-flag"])
        .env("ANTHROPIC_OAUTH_TOKEN", "synthetic-env-shadow-fixture")
        .output()
        .unwrap();
    assert_native_usage_parser(mixed, "environment credential with supported default file");
}

#[test]
#[cfg(unix)]
fn configured_antigravity_does_not_force_the_usage_route_to_go() {
    let root = TempDir::new().unwrap();
    let credentials = root
        .path()
        .join("home")
        .join(".claude")
        .join(".credentials.json");
    std::fs::create_dir_all(credentials.parent().unwrap()).unwrap();
    std::fs::write(
        &credentials,
        br#"{"oauthAccount":{"default":{"accessToken":"synthetic-claude-file-fixture"}}}"#,
    )
    .unwrap();
    let fake_bin = root.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake_ps = fake_bin.join("ps");
    std::fs::write(
        &fake_ps,
        "#!/bin/sh\nprintf '%s\\n' ' 445 /Applications/Antigravity.app/Contents/Resources/language_server --app_data_dir antigravity'\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_ps).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_ps, permissions).unwrap();

    // Antigravity is always configured. The usage route must no longer
    // delegate solely because its local server may be running. The synthetic
    // ps command reports that server without inspecting real processes; the
    // invalid flag stops before Service::new can enumerate anything else.
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .env("PATH", fake_bin)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
        "native usage parser did not handle the request: {stderr}"
    );
    assert!(
        !stderr.contains("no Go fallback") && !stderr.contains("Go fallback"),
        "Antigravity unexpectedly selected Go fallback: {stderr}"
    );
}

#[test]
fn noncanonical_or_ambiguous_claude_files_remain_on_go() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../symbrain-usage/tests/fixtures/claude_file_token_oracle.json"
    ))
    .expect("Go Claude parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let id = case["id"].as_str().expect("case id");
        if matches!(
            id,
            "default-account-precedes-other-accounts"
                | "single-nondefault-account-is-unambiguous"
                | "duplicate-account-key-uses-last-token"
        ) {
            continue;
        }
        let root = TempDir::new().unwrap();
        let credentials = root
            .path()
            .join("home")
            .join(".claude")
            .join(".credentials.json");
        std::fs::create_dir_all(credentials.parent().unwrap()).unwrap();
        std::fs::write(
            &credentials,
            case["contents"].as_str().expect("file contents"),
        )
        .unwrap();

        // Invalid syntax returns before fetching or accessing credentials. An
        // existing file whose Go interpretation is broader or nondeterministic
        // must select Go before the native parser or any Keychain read.
        let output = command(&root, &["usage", "--not-a-usage-flag"])
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("not ported yet and no Go fallback was found"),
            "Claude file case {id} should remain on Go: {stderr}"
        );
    }
}

#[test]
fn command_factory_hermetically_isolates_environment_from_outer_xdg_and_provider_keys() {
    let current_exe = std::env::current_exe().expect("current test executable path");
    let outer_data_dir = TempDir::new().unwrap();
    let outer_audit_dir = outer_data_dir.path().join("symbrain").join("audit");

    let mut child = Command::new(current_exe)
        .args([
            "--exact",
            "native_mcp_audit_creates_redacted_jsonl_without_stdout_pollution",
            "--nocapture",
        ])
        .env("XDG_DATA_HOME", outer_data_dir.path())
        .env("ANTHROPIC_API_KEY", "inert-sentinel-test-token")
        .env("OPENAI_API_KEY", "inert-sentinel-test-token")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn child test runner");

    let start = Instant::now();
    let timeout = Duration::from_secs(30);
    let mut timed_out = false;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed to wait on child process: {err}");
            }
        }
    }

    let output = child
        .wait_with_output()
        .expect("failed to collect child output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !timed_out,
        "child test runner timed out after {timeout:?}.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "child test runner failed with status {:?}.\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        !outer_audit_dir.exists(),
        "outer audit directory was created in outer XDG_DATA_HOME, proving environment leaked into child command"
    );
}

#[test]
fn codex_default_file_env_and_home_override_routes_are_native() {
    let root = TempDir::new().unwrap();
    install_synthetic_claude_file(&root);
    let default_auth = root.path().join("home").join(".codex").join("auth.json");
    std::fs::create_dir_all(default_auth.parent().unwrap()).unwrap();
    std::fs::write(
        &default_auth,
        br#"{"access_token":"synthetic-codex-file-fixture"}"#,
    )
    .unwrap();

    // Invalid CLI syntax exits before usage fetching. This proves the default
    // auth.json selects native routing without contacting chatgpt.com.
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
        "native usage parser did not handle Codex auth.json: {stderr}"
    );
    assert!(
        !stderr.contains("no Go fallback") && !stderr.contains("Go fallback"),
        "Codex auth.json unexpectedly selected Go fallback: {stderr}"
    );
    let empty_override = command(&root, &["usage", "--not-a-usage-flag"])
        .env("CODEX_HOME", "")
        .output()
        .unwrap();
    assert_native_usage_parser(empty_override, "empty CODEX_HOME uses the default home");

    let override_home = root.path().join("codex-override");
    std::fs::create_dir_all(&override_home).unwrap();
    std::fs::write(
        override_home.join("auth.json"),
        br#"{"tokens":{"access_token":"synthetic-codex-override-fixture"}}"#,
    )
    .unwrap();
    let overridden = command(&root, &["usage", "--not-a-usage-flag"])
        .env("CODEX_HOME", &override_home)
        .output()
        .unwrap();
    assert_native_usage_parser(overridden, "CODEX_HOME override");

    let mixed = command(&root, &["usage", "--not-a-usage-flag"])
        .env("CODEX_ACCESS_TOKEN", "synthetic-codex-env-fixture")
        .output()
        .unwrap();
    assert_native_usage_parser(mixed, "environment credential with supported default file");

    std::fs::write(
        &default_auth,
        br#"{"access_token":"symvault://codex/access-token"}"#,
    )
    .unwrap();
    let reference = command(&root, &["usage", "--not-a-usage-flag"])
        .output()
        .unwrap();
    let reference_stderr = String::from_utf8(reference.stderr).unwrap();
    assert!(
        reference_stderr.contains("not ported yet and no Go fallback was found"),
        "reference-shaped file credential must stay on Go: {reference_stderr}"
    );
}

#[test]
fn kimi_and_nous_supported_home_overrides_route_natively_only_for_proven_files() {
    let default_kimi_root = TempDir::new().unwrap();
    install_synthetic_claude_file(&default_kimi_root);
    let default_kimi_file = default_kimi_root
        .path()
        .join("home/.kimi-code/credentials/kimi-code.json");
    std::fs::create_dir_all(default_kimi_file.parent().unwrap()).unwrap();
    std::fs::write(
        &default_kimi_file,
        br#"{"access_token":"synthetic-kimi-default"}"#,
    )
    .unwrap();
    let default_kimi = command(&default_kimi_root, &["usage", "--not-a-usage-flag"])
        .env("KIMI_CODE_HOME", "")
        .output()
        .unwrap();
    assert_native_usage_parser(default_kimi, "empty KIMI_CODE_HOME uses the default home");

    let kimi_root = TempDir::new().unwrap();
    install_synthetic_claude_file(&kimi_root);
    let kimi_home = kimi_root.path().join("kimi-override");
    let kimi_credentials = kimi_home.join("credentials/kimi-code.json");
    std::fs::create_dir_all(kimi_credentials.parent().unwrap()).unwrap();
    std::fs::write(
        &kimi_credentials,
        br#"{"access_token":"synthetic-kimi-home-override"}"#,
    )
    .unwrap();
    std::fs::write(kimi_home.join("device_id"), b"synthetic-device-id\n").unwrap();
    let kimi = command(&kimi_root, &["usage", "--not-a-usage-flag"])
        .env("KIMI_CODE_HOME", &kimi_home)
        .output()
        .unwrap();
    assert_native_usage_parser(kimi, "KIMI_CODE_HOME with canonical token and device id");

    let nous_root = TempDir::new().unwrap();
    install_synthetic_claude_file(&nous_root);
    let default_nous_file = nous_root.path().join("home/.hermes/auth.json");
    std::fs::create_dir_all(default_nous_file.parent().unwrap()).unwrap();
    std::fs::write(
        &default_nous_file,
        br#"{"providers":[{"id":"nous","access_token":"synthetic-nous-default"}]}"#,
    )
    .unwrap();
    let default_nous = command(&nous_root, &["usage", "--not-a-usage-flag"])
        .env("HERMES_HOME", "")
        .output()
        .unwrap();
    assert_native_usage_parser(default_nous, "empty HERMES_HOME uses the default home");

    let nous_home = nous_root.path().join("hermes-override");
    std::fs::create_dir_all(&nous_home).unwrap();
    std::fs::write(
        nous_home.join("auth.json"),
        br#"{"providers":[{"id":"nous","invoke_jwt":"header.eyJleHAiOjQxMDI0NDQ4MDB9.signature"}]}"#,
    )
    .unwrap();
    let nous = command(&nous_root, &["usage", "--not-a-usage-flag"])
        .env("HERMES_HOME", &nous_home)
        .output()
        .unwrap();
    assert_native_usage_parser(nous, "HERMES_HOME with canonical live JWT");

    std::fs::write(
        nous_home.join("auth.json"),
        br#"{"providers":[{"id":"nous","invoke_jwt":"header.a.b"}]}"#,
    )
    .unwrap();
    let malformed = command(&nous_root, &["usage", "--not-a-usage-flag"])
        .env("HERMES_HOME", &nous_home)
        .output()
        .unwrap();
    let stderr = String::from_utf8(malformed.stderr).unwrap();
    assert!(
        stderr.contains("not ported yet and no Go fallback was found"),
        "malformed existing Hermes JWT must remain on Go: {stderr}"
    );
}

#[test]
fn copilot_single_default_file_routes_natively_and_unproven_shapes_keep_go() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../symbrain-usage/tests/fixtures/copilot_file_token_oracle.json"
    ))
    .expect("Go Copilot parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let id = case["id"].as_str().expect("case id");
        let root = TempDir::new().unwrap();
        let config = root.path().join("home/.config/github-copilot");
        for (key, filename) in [("apps_json", "apps.json"), ("hosts_json", "hosts.json")] {
            if let Some(contents) = case[key].as_str() {
                std::fs::create_dir_all(&config).unwrap();
                std::fs::write(config.join(filename), contents).unwrap();
            }
        }
        let output = command(&root, &["usage", "--not-a-usage-flag"])
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        if case["native_route"] == true {
            assert_eq!(output.status.code(), Some(2), "{id}: {stderr}");
            assert!(
                stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
                "Go-equivalent Copilot file case {id} should select native parser: {stderr}"
            );
            assert!(
                !stderr.contains("no Go fallback") && !stderr.contains("Go fallback"),
                "Copilot file case {id} unexpectedly selected Go fallback: {stderr}"
            );
        } else {
            assert!(
                stderr.contains("not ported yet and no Go fallback was found"),
                "unproven Copilot file case {id} must remain on Go: {stderr}"
            );
        }
    }

    let root = TempDir::new().unwrap();
    let apps = root.path().join("home/.config/github-copilot/apps.json");
    std::fs::create_dir_all(apps.parent().unwrap()).unwrap();
    std::fs::write(
        &apps,
        br#"{"github.com:Iv1.synthetic":{"oauth_token":"synthetic-copilot-file-fixture"}}"#,
    )
    .unwrap();
    let mixed = command(&root, &["usage", "--not-a-usage-flag"])
        .env("COPILOT_ACCESS_TOKEN", "synthetic-copilot-env-fixture")
        .output()
        .unwrap();
    assert_native_usage_parser(mixed, "Copilot environment credential with default file");
}

#[test]
fn nous_auth_file_presence_keeps_unproven_shapes_on_go() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../symbrain-usage/tests/fixtures/nous_file_token_oracle.json"
    ))
    .expect("Go Nous parser oracle");
    let cases = fixture["cases"].as_array().expect("oracle cases");
    assert!(cases.iter().any(|case| {
        case["id"] == "case-insensitive-struct-fields"
            && case["token"] == "synthetic-nous-case-token"
    }));
    for id in [
        "missing-file",
        "empty-file",
        "malformed-json",
        "wrong-typed-token-invalidates-file",
        "unrelated-provider",
    ] {
        assert!(
            cases
                .iter()
                .any(|case| case["id"] == id && case["token"] == "")
        );
    }
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let root = TempDir::new().unwrap();
        let auth = root.path().join("home/.hermes/auth.json");
        if case["file_present"] == true {
            std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
            std::fs::write(
                &auth,
                case["contents"].as_str().expect("present file contents"),
            )
            .unwrap();
        }

        let output = command(&root, &["usage", "--not-a-usage-flag"])
            .output()
            .unwrap();
        if case["file_present"] != true || case["native_route"] == true {
            assert_native_usage_parser(output, id);
        } else {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains("not ported yet and no Go fallback was found"),
                "unproven Nous auth.json case {id} must remain on Go: {stderr}"
            );
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root = TempDir::new().unwrap();
        let auth = root.path().join("home/.hermes/auth.json");
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        symlink(root.path().join("missing-target"), &auth).unwrap();
        let output = command(&root, &["usage", "--not-a-usage-flag"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("not ported yet and no Go fallback was found"),
            "dangling Nous auth.json symlink must remain on Go"
        );
    }
}

#[cfg(windows)]
#[test]
fn differing_home_and_userprofile_keep_the_whole_usage_report_on_go() {
    let root = TempDir::new().unwrap();
    let rust_home = root.path().join("rust-home");
    let go_home = root.path().join("userprofile-home");
    let auth = go_home.join(".hermes/auth.json");
    std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
    std::fs::write(
        &auth,
        br#"{"providers":[{"id":"nous","invoke_jwt":"synthetic-nous-windows-fixture"}]}"#,
    )
    .unwrap();

    // Go resolves its default home through USERPROFILE on Windows; Rust's
    // current home helper uses HOME. A second direct credential confirms the
    // mismatch keeps the entire report on Go, not only the Nous row.
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .env("HOME", rust_home)
        .env("USERPROFILE", go_home)
        .env("OPENROUTER_API_KEY", "synthetic-direct-env-fixture")
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("not ported yet and no Go fallback was found"),
        "different Windows home roots must retain Go routing: {stderr}"
    );
}

fn assert_native_usage_parser(output: Output, case: &str) {
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(2), "{case}: {stderr}");
    assert!(
        stderr.starts_with("flag provided but not defined: -not-a-usage-flag\n"),
        "{case} did not reach the native parser before fetching: {stderr}"
    );
}

#[test]
fn kimi_default_file_routes_follow_the_source_bound_candidate_contract() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../symbrain-usage/tests/fixtures/kimi_file_token_oracle.json"
    ))
    .expect("Go Kimi parser oracle");
    for case in fixture["cases"].as_array().expect("Go cases") {
        let id = case["id"].as_str().expect("case id");
        let root = TempDir::new().unwrap();
        let auth = root
            .path()
            .join("home/.kimi-code/credentials/kimi-code.json");
        if case["file_present"] == true {
            std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
            std::fs::write(&auth, case["contents"].as_str().unwrap()).unwrap();
        }
        // Invalid syntax exits before credential resolution or provider requests.
        let output = command(&root, &["usage", "--not-a-usage-flag"])
            .output()
            .unwrap();
        if case["file_present"] != true || id == "canonical-with-ignored-refresh-token" {
            assert_native_usage_parser(output, id);
        } else {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains("not ported yet and no Go fallback was found"),
                "unproven Kimi case {id} must remain on Go: {stderr}"
            );
        }
    }
}

#[test]
fn supported_provider_combination_routes_before_any_live_request() {
    let root = TempDir::new().unwrap();
    let output = command(&root, &["usage", "--not-a-usage-flag"])
        .env("KIMI_CODE_API_KEY", "synthetic-kimi-api")
        .env("KIMI_AUTH_TOKEN", "synthetic-kimi-web")
        .env("KIMI_CODE_BASE_URL", "https://api.kimi.com/custom/v1")
        .env("MOONSHOT_API_KEY", "synthetic-moonshot")
        .env("MOONSHOT_REGION", "cn")
        .env("NOUS_PORTAL_ACCESS_TOKEN", "synthetic-nous")
        .env("OPENCODE_COOKIE", "synthetic-opencode")
        .env("OPENCODE_WORKSPACE_ID", "wrk_fixture123")
        .env("OPENROUTER_API_KEY", "synthetic-openrouter")
        .env("OPENROUTER_API_URL", "https://openrouter.ai/custom/v1")
        .output()
        .unwrap();
    assert_native_usage_parser(output, "supported provider combination");
}
