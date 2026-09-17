//! Focused native sync slice and fallback routing checks.

use std::process::{Command, Output};

use tempfile::TempDir;

fn run(root: &TempDir, args: &[&str]) -> Output {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let project = root.path().join("project");
    for path in [&home, &config, &data, &cache, &project] {
        std::fs::create_dir_all(path).unwrap();
    }

    Command::new(env!("CARGO_BIN_EXE_symbrain"))
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
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn instruction_only_target_is_native_and_matches_sync_schema() {
    let root = TempDir::new().unwrap();
    let source_dir = root.path().join("config/symbrain");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::write(source_dir.join("instructions.md"), b"global\n").unwrap();
    let project_source = root.path().join("project/.symbrain");
    std::fs::create_dir_all(&project_source).unwrap();
    std::fs::write(project_source.join("instructions.md"), b"project\n").unwrap();

    let output = run(&root, &["sync", "agents", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["skills"][0]["target"], "agents");
    assert_eq!(value["skills"][0]["status"], "skipped");
    assert_eq!(
        value["skills"][0]["message"],
        "no skill target for harness \"agents\""
    );
    assert_eq!(value["targets"][0]["name"], "agents");
    assert_eq!(value["targets"][0]["status"], "created");
    assert_eq!(value["targets"][0]["path"].as_str(), Some("AGENTS.md"));

    let target = std::fs::read(root.path().join("project/AGENTS.md")).unwrap();
    let target = String::from_utf8(target).unwrap();
    assert!(target.contains("global\nproject\n"));
    assert!(target.contains("<!-- symbrain:begin -->"));
    assert!(target.contains("<!-- symbrain:end -->"));
}

#[test]
fn skill_target_remains_on_go_fallback_before_output() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["sync", "claude", "--dry-run"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn project_override_remains_on_go_fallback_before_output() {
    let root = TempDir::new().unwrap();
    let project = root.path().join("alternate-project");
    let output = run(
        &root,
        &["sync", "--project", project.to_str().unwrap(), "agents"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}
