//! Byte-level subprocess coverage for the native `symbrain harness list` CLI.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::process::{Command, Output};

use tempfile::TempDir;

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

fn run(root: &TempDir, args: &[&str]) -> Output {
    command(root, args).output().unwrap()
}

fn write_claude_config(root: &TempDir, contents: &[u8]) {
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    let path = root.path().join("home/.claude.json");
    std::fs::write(path, contents).unwrap();
}

#[test]
fn json_is_compact_and_preserves_inventory_schema() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["harness", "list", "--json"]);

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert!(
        !output.stdout[..output.stdout.len() - 1].contains(&b'\n'),
        "JSON must be one compact line"
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["harnesses"][0]["name"], "claude");
    assert_eq!(value["harnesses"][0]["global"]["exists"], false);
}

#[test]
fn table_matches_go_layout_for_populated_and_missing_configs() {
    let root = TempDir::new().unwrap();
    write_claude_config(
        &root,
        br#"{"mcpServers":{"zeta":{"command":"zcmd","args":["--x"],"env":{"TOKEN":"secret"}},"alpha":{"url":"https://example.test/mcp"}}}"#,
    );
    let output = run(&root, &["harness", "list"]);

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    let config = root.join("config");
    let claude_desktop_config = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    } else if cfg!(target_os = "windows") {
        home.join("AppData/Roaming/Claude/claude_desktop_config.json")
    } else {
        config.join("Claude/claude_desktop_config.json")
    };
    let expected = format!(
        "claude\tClaude Code\n  global\t{}\tparsed\tservers=alpha[http],zeta[stdio]\n\n\
claude-desktop\tClaude Desktop\n  global\t{}\tmissing\tservers=(none)\n\n\
cursor\tCursor\n  global\t{}\tmissing\tservers=(none)\n\n\
opencode\tOpenCode\n  global\t{}\tmissing\tservers=(none)\n\n\
codex\tCodex CLI\n  global\t{}\tmissing\tservers=(none)\n\n\
antigravity\tAntigravity\n  global\t{}\tmissing\tservers=(none)\n\n",
        home.join(".claude.json").display(),
        claude_desktop_config.display(),
        home.join(".cursor/mcp.json").display(),
        config.join("opencode/config.json").display(),
        home.join(".codex/config.toml").display(),
        home.join(".gemini/config/mcp_config.json").display(),
    );
    assert_eq!(stdout, expected);
}

#[test]
fn table_reports_parsed_global_and_project_configs() {
    let root = TempDir::new().unwrap();
    write_claude_config(
        &root,
        br#"{"mcpServers":{"global":{"command":"global-cmd"}}}"#,
    );
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".mcp.json"),
        br#"{"mcpServers":{"local":{"command":"local-cmd"}}}"#,
    )
    .unwrap();
    let project = project.canonicalize().unwrap();
    let output = run(
        &root,
        &["harness", "list", "--project", project.to_str().unwrap()],
    );

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("  global\t"));
    assert!(stdout.contains("\tparsed\tservers=global[stdio]\n"));
    assert!(stdout.contains("  project\t"));
    assert!(stdout.contains("\tparsed\tservers=local[stdio]\n\n"));
}

#[test]
fn invalid_and_extra_arguments_keep_go_exit_and_diagnostics() {
    let root = TempDir::new().unwrap();
    let extra = run(&root, &["harness", "list", "extra"]);
    assert_eq!(extra.status.code(), Some(2));
    assert!(extra.stdout.is_empty());
    assert_eq!(
        extra.stderr,
        b"symbrain harness list: unexpected argument \"extra\"\n"
    );

    let missing = run(&root, &["harness"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    assert_eq!(
        String::from_utf8(missing.stderr).unwrap(),
        "symbrain harness — inspect configured AI harnesses\n\nUsage:\n  symbrain harness list [--project DIR]\n  symbrain harness health [--harness NAME] [--project DIR]\n\nThe global --output table|json flag (or --json) selects the output format.\n"
    );
}

#[test]
fn malformed_json_inventory_is_native_without_go_fallback() {
    let root = TempDir::new().unwrap();
    write_claude_config(&root, b"{not-json");
    let missing_go = root.path().join("missing-go-fallback");
    let mut table_command = command(&root, &["harness", "list"]);
    table_command.env("SYMBRAIN_GO_BINARY", &missing_go);
    let table = table_command.output().unwrap();

    assert!(table.status.success(), "stderr: {:?}", table.stderr);
    assert!(table.stderr.is_empty());
    let stdout = String::from_utf8(table.stdout).unwrap();
    assert!(stdout.contains("\tinvalid\tservers=(none)\n"));
    assert!(stdout.contains(
        "error: harness: claude config is not valid json; refusing to edit a config symbrain cannot parse: parse json: invalid character 'n'\n"
    ));

    let mut json_command = command(&root, &["harness", "list", "--json"]);
    json_command.env("SYMBRAIN_GO_BINARY", &missing_go);
    let json = json_command.output().unwrap();
    assert!(json.status.success(), "stderr: {:?}", json.stderr);
    assert!(json.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        value["harnesses"][0]["global"]["error"],
        "harness: claude config is not valid json; refusing to edit a config symbrain cannot parse: parse json: invalid character 'n'"
    );
}

#[test]
fn malformed_config_health_is_native_without_go_fallback() {
    let root = TempDir::new().unwrap();
    write_claude_config(&root, b"{not-json");
    let missing_go = root.path().join("missing-go-fallback");
    for (args, expected) in [
        (
            &["harness", "health"][..],
            b"no MCP servers found\n".as_slice(),
        ),
        (
            &["harness", "health", "--json"][..],
            b"{\"servers\":null}\n".as_slice(),
        ),
    ] {
        let mut command = command(&root, args);
        command.env("SYMBRAIN_GO_BINARY", &missing_go);
        let output = command.output().unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn missing_command_health_reports_redacted_error_without_go_fallback() {
    let root = TempDir::new().unwrap();
    write_claude_config(
        &root,
        br#"{"mcpServers":{"missing":{"command":"symaira-missing-mcp-fixture"}}}"#,
    );
    let mut command = command(&root, &["harness", "health", "--json"]);
    command.env(
        "SYMBRAIN_GO_BINARY",
        root.path().join("missing-go-fallback"),
    );
    let output = command.output().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let server = &report["servers"][0];
    // #598 intentionally replaces path- and environment-rich broker errors
    // with a stable, bounded diagnostic.
    assert_eq!(server["outcome"], "unhealthy");
    assert_eq!(server["error"], "server command was not found");
    assert!(server.get("probe_method").is_none());
    assert!(server.get("latency_ms").is_none());
}

#[test]
fn successful_health_probe_is_native_without_go_fallback() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("probe.toml");
    std::fs::write(&profile, b"[profile]\nname = \"probe\"\n").unwrap();
    let child = env!("CARGO_BIN_EXE_symbrain");
    let config = serde_json::json!({"mcpServers":{"probe":{
        "command":child,
        "args":["mcp","--profile-file",profile]
    }}});
    write_claude_config(&root, serde_json::to_string(&config).unwrap().as_bytes());
    let mut command = command(&root, &["harness", "health", "--json"]);
    command.env(
        "SYMBRAIN_GO_BINARY",
        root.path().join("missing-go-fallback"),
    );
    let output = command.output().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["servers"][0]["server"], "probe");
    assert_eq!(report["servers"][0]["healthy"], true);
}

#[cfg(unix)]
#[test]
fn toml_and_io_inventory_errors_keep_go_fallback_before_output() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    let root = TempDir::new().unwrap();
    let codex = root.path().join("home/.codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(codex.join("config.toml"), b"mcp_servers = [\n").unwrap();
    let fallback = root.path().join("go-fallback");
    std::fs::write(
        &fallback,
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 17\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fallback).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fallback, permissions).unwrap();

    let mut fallback_command = command(&root, &["harness", "list", "--json"]);
    fallback_command.env("SYMBRAIN_GO_BINARY", &fallback);
    let output = fallback_command.output().unwrap();

    assert_eq!(output.status.code(), Some(17));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");

    write_claude_config(&root, b"[]");
    let mut json_command = command(&root, &["harness", "list", "--json"]);
    json_command.env("SYMBRAIN_GO_BINARY", &fallback);
    let json_output = json_command.output().unwrap();
    assert_eq!(json_output.status.code(), Some(17));
    assert_eq!(json_output.stdout, b"fallback-stdout\n");
    assert_eq!(json_output.stderr, b"fallback-stderr\n");

    write_claude_config(
        &root,
        br#"{"mcpServers":{"global":{"command":"global-cmd"}}}"#,
    );
    std::fs::write(root.path().join("unsafe.json"), b"{}").unwrap();
    symlink(
        root.path().join("unsafe.json"),
        root.path().join("project/.mcp.json"),
    )
    .unwrap();
    let mut project_command = command(
        &root,
        &[
            "harness",
            "list",
            "--json",
            "--project",
            root.path()
                .join("project")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
        ],
    );
    project_command.env("SYMBRAIN_GO_BINARY", &fallback);
    let project_output = project_command.output().unwrap();

    assert_eq!(project_output.status.code(), Some(17));
    assert_eq!(project_output.stdout, b"fallback-stdout\n");
    assert_eq!(project_output.stderr, b"fallback-stderr\n");
}
