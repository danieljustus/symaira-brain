#![deny(unsafe_code)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fs;
use std::os::unix::fs::PermissionsExt;
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
        .envs(coverage::profile_environment())
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
fn doctor_detects_hermes_yaml_config_and_never_prints_an_empty_path() {
    let root = TempDir::new().unwrap();
    let profiles_dir = root.path().join("config/symbrain/profiles");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::create_dir_all(root.path().join("home/.hermes")).unwrap();
    fs::write(
        profiles_dir.join("personal.toml"),
        "[profile]\nname = \"personal\"\n\n[servers.vault]\nenabled = false\n",
    )
    .unwrap();
    fs::write(
        root.path().join("home/.hermes/config.yaml"),
        "mcp_servers:\n  symbrain:\n    command: /opt/bin/symbrain\n    args: [mcp, -profile, personal, --vault-agent, hermes]\n",
    )
    .unwrap();

    let json = command(&root, &["doctor", "--json"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let hermes = report["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|harness| harness["name"] == "hermes")
        .unwrap();
    assert_eq!(hermes["installed"], json!(true));
    assert_eq!(hermes["profile"], json!("personal"));
    assert!(
        hermes["config_path"]
            .as_str()
            .unwrap()
            .ends_with("home/.hermes/config.yaml")
    );
    let registered = report["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["name"] == "profile \"personal\": registered")
        .unwrap();
    assert_eq!(registered["status"], json!("pass"));

    let human = command(&root, &["doctor"]).output().unwrap();
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("hermes         installed, profile \"personal\""),
        "{text}"
    );
    assert!(
        text.contains("agents         unsupported (no MCP config path)"),
        "{text}"
    );
    assert!(!text.contains("config not found: \n"), "{text}");
}

#[test]
fn doctor_flags_managed_core_version_mismatch_with_setup_fix_hint() {
    let root = TempDir::new().unwrap();
    let managed = root.path().join("home/.symaira/bin");
    fs::create_dir_all(&managed).unwrap();
    let binary = managed.join("symvault");
    fs::write(&binary, "#!/bin/sh\nprintf '{\"version\":\"0.0.1\"}\\n'\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

    let json = command(&root, &["doctor", "--json"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let core = report["managed_cores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|core| core["name"] == "symvault")
        .unwrap();
    assert_eq!(core["version"], json!("0.0.1"));
    assert_eq!(core["mismatch"], json!(true));

    let human = command(&root, &["doctor"]).output().unwrap();
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.lines().any(|line| line.starts_with("    !  symvault")
            && line.contains("run `symbrain setup --fix`")),
        "{text}"
    );
}

fn symvault_core(root: &TempDir) -> serde_json::Value {
    let json = command(root, &["doctor", "--json"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    report["managed_cores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|core| core["name"] == "symvault")
        .unwrap()
        .clone()
}

#[test]
fn doctor_flags_a_provenance_sidecar_that_no_longer_matches_the_binary() {
    use sha2::{Digest, Sha256};

    let root = TempDir::new().unwrap();
    let managed = root.path().join("home/.symaira/bin");
    fs::create_dir_all(&managed).unwrap();
    let pinned = symvault_core(&root)["pinned"].as_str().unwrap().to_string();
    let binary = managed.join("symvault");
    let script = format!("#!/bin/sh\nprintf '{{\"version\":\"{pinned}\"}}\\n'\n");
    fs::write(&binary, &script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let human_without_sidecar = command(&root, &["doctor"]).output().unwrap().stdout;
    let json_without_sidecar = command(&root, &["doctor", "--json"])
        .output()
        .unwrap()
        .stdout;

    let sidecar = managed.join("symvault.provenance.json");
    let write_sidecar = |sha: &str| {
        let record = json!({
            "binary": "symvault",
            "source": "release",
            "version": pinned,
            "repo": "danieljustus/symvault",
            "built_at": "2026-10-02T10:00:00Z",
            "binary_sha256": sha,
        });
        fs::write(&sidecar, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    };

    // A sidecar describing the installed binary changes nothing.
    write_sidecar(&format!("{:x}", Sha256::digest(script.as_bytes())));
    assert_eq!(
        command(&root, &["doctor"]).output().unwrap().stdout,
        human_without_sidecar
    );
    assert_eq!(
        command(&root, &["doctor", "--json"])
            .output()
            .unwrap()
            .stdout,
        json_without_sidecar
    );

    // A sidecar left behind by an out-of-band upgrade is reported.
    write_sidecar(&"0".repeat(64));
    let core = symvault_core(&root);
    assert_eq!(core["mismatch"], json!(null));
    assert_eq!(core["provenance_stale"], json!(true));
    let text = String::from_utf8(command(&root, &["doctor"]).output().unwrap().stdout).unwrap();
    assert!(
        text.lines().any(|line| line.starts_with("    !  symvault")
            && line.contains("symvault.provenance.json does not describe the installed binary")),
        "{text}"
    );
}

#[test]
fn doctor_json_true_uses_json_output() {
    let root = TempDir::new().unwrap();
    let output = command(&root, &["doctor", "--json=true"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.get("config").is_some());
}

#[test]
fn doctor_vault_probe_does_not_expose_child_stderr() {
    const SECRET: &str = "credential=SENTINEL_SECRET_473";
    let root = TempDir::new().unwrap();
    let vault_dir = root.path().join("fake-vault-bin");
    fs::create_dir_all(&vault_dir).unwrap();
    let vault = vault_dir.join("symvault");
    fs::write(
        &vault,
        format!("#!/bin/sh\nprintf '%s\\n' '{SECRET}' >&2\nexit 42\n"),
    )
    .unwrap();
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o755)).unwrap();

    for args in [&["doctor"][..], &["doctor", "--json"][..]] {
        let output = command(&root, args)
            .env("PATH", &vault_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stdout.contains(SECRET),
            "doctor stdout leaked child stderr: {stdout}"
        );
        assert!(
            !stderr.contains(SECRET),
            "doctor stderr leaked child stderr: {stderr}"
        );
        assert!(output.stderr.is_empty(), "unexpected stderr: {stderr}");

        if args.len() == 2 {
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let vault_link = report["links"]
                .as_array()
                .unwrap()
                .iter()
                .find(|link| link["name"] == "vault: reachable")
                .unwrap();
            assert_eq!(vault_link["status"], "fail");
            assert_eq!(
                vault_link["detail"],
                "symvault probe failed: exit status 42"
            );
        }
    }
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
