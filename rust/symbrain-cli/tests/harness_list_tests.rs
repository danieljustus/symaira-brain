//! Byte-level subprocess coverage for the native `symbrain harness list` CLI.

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

#[cfg(unix)]
#[test]
fn malformed_inventory_falls_back_before_native_output() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new().unwrap();
    write_claude_config(&root, b"{not-json");
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

    write_claude_config(
        &root,
        br#"{"mcpServers":{"global":{"command":"global-cmd"}}}"#,
    );
    std::fs::write(root.path().join("project/.mcp.json"), b"{not-json").unwrap();
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
