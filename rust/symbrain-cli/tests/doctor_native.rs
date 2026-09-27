#![cfg(unix)]
#![deny(unsafe_code)]

use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let project = root.path().join("project");
    let empty_path = root.path().join("empty-path");
    for path in [&home, &config, &data, &cache, &project, &empty_path] {
        fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", &empty_path)
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .args(args);
    command
}

#[test]
fn doctor_json_reports_static_config_and_registered_harness_natively() {
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("config/symbrain");
    let profiles_dir = config_dir.join("profiles");
    fs::create_dir_all(root.path().join("home")).unwrap();
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        "default_profile = \"personal\"\n",
    )
    .unwrap();
    fs::write(
        profiles_dir.join("personal.toml"),
        "[profile]\nname = \"personal\"\n\n[servers.vault]\nenabled = false\n",
    )
    .unwrap();
    fs::write(
        root.path().join("home/.claude.json"),
        r#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","personal"]}}}"#,
    )
    .unwrap();

    let output = command(&root, &["doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["config"]["exists"], json!(true));
    assert_eq!(report["config"]["parsed"], json!(true));
    assert_eq!(report["builtins"], json!(["memory", "skills", "usage"]));
    assert_eq!(report["profiles"], json!(["personal"]));

    let claude = report["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|harness| harness["name"] == "claude")
        .unwrap();
    assert_eq!(claude["config_found"], json!(true));
    assert_eq!(claude["config_parsed"], json!(true));
    assert_eq!(claude["installed"], json!(true));
    assert_eq!(claude["profile"], json!("personal"));
    assert_eq!(claude["profile_exists"], json!(true));
    assert_eq!(claude["profile_missing"], json!(false));

    let registered = report["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["name"] == "profile \"personal\": registered")
        .unwrap();
    assert_eq!(registered["status"], json!("pass"));
}

#[test]
fn doctor_vault_agent_without_profiles_stays_native_with_invalid_go_binary() {
    let root = TempDir::new().unwrap();
    let output = command(&root, &["doctor", "--vault-agent", "agent"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout.first(), Some(&b's'));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("symbrain doctor\n"));
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
}

#[test]
fn doctor_vault_agent_skips_broken_profile_without_go() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    fs::create_dir_all(&profiles).unwrap();
    fs::write(profiles.join("broken.toml"), b"[profile\n").unwrap();
    let output = command(&root, &["doctor", "--vault-agent", "agent", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["profiles"], json!(["broken"]));
    assert!(report.get("handshakes").is_none());
}

#[test]
fn doctor_vault_agent_with_disabled_vault_profile_stays_native() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    fs::create_dir_all(&profiles).unwrap();
    fs::write(
        profiles.join("personal.toml"),
        b"[profile]\nname = \"personal\"\n[servers.vault]\nenabled = false\n",
    )
    .unwrap();

    let output = command(&root, &["doctor", "--vault-agent", "other", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.get("handshakes").is_none());
}

#[test]
fn doctor_vault_agent_with_missing_vault_binary_stays_native() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    fs::create_dir_all(&profiles).unwrap();
    fs::write(
        profiles.join("personal.toml"),
        b"[profile]\nname = \"personal\"\n[servers.vault]\nenabled = true\n",
    )
    .unwrap();

    let output = command(&root, &["doctor", "--vault-agent", "other", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["handshakes"][0]["profile"], json!("personal"));
    assert_eq!(report["handshakes"][0]["server"], json!("vault"));
    assert!(
        report["handshakes"][0]["error"]
            .as_str()
            .unwrap()
            .contains("not found on PATH or in managed directory")
    );
}

#[test]
fn doctor_vault_agent_skips_unlistable_profiles_without_go() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    fs::create_dir_all(profiles.parent().unwrap()).unwrap();
    fs::write(&profiles, b"profiles are not a directory").unwrap();
    let output = command(&root, &["doctor", "--vault-agent", "agent", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["profiles"], json!([]));
    assert!(report.get("handshakes").is_none());
}
