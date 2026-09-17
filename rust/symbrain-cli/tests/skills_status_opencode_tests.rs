//! CLI byte checks for the native `OpenCode` user-scope status slice.

use std::process::{Command, Output};

use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let project = root.path().join("project");
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

#[test]
fn opencode_user_status_empty_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"No installed skills found.\n");
}

#[test]
fn opencode_user_status_populated_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let skill = root.path().join("home/.config/opencode/skills/handwritten");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), b"handwritten\n").unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let expected = format!(
        "TARGET\tSKILL\tSTATUS\tMODE\tPATH\nopencode\thandwritten\tunmanaged\t-\t{}\n",
        skill.display()
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn opencode_user_status_empty_json_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(
        &root,
        &["skills", "status", "--target", "opencode", "--json"],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"{\"installs\":[],\"summary\":{\"in_sync\":0,\"stale\":0,\"harness_changed\":0,\"conflict\":0,\"orphaned\":0,\"unmanaged\":0}}\n"
    );
}

#[test]
fn unsupported_status_target_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status", "--target", "claude"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn configured_status_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        b"library_dir = \"/different/library\"\n",
    )
    .unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn legacy_go_config_wins_when_new_skills_config_dir_exists() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("config/symbrain/skills")).unwrap();
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        b"library_dir = \"/different/library\"\n",
    )
    .unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn symskills_config_override_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let mut command = command(&root, &["skills", "status", "--target", "opencode"]);
    let output = command
        .env("SYMSKILLS_LIBRARY_DIR", "/different/library")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}
