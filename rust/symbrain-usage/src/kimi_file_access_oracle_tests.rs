//! Go-derived oracle for Kimi CLI credential filesystem shapes (#768).
//!
//! `scripts/usage-oracle` builds each case in a disposable HOME and records what
//! the shipped Go `NewKimiProvider` observed. This test recreates the same tree
//! and runs the *native* routing and provider constructor in a child process
//! with a cleared environment, so process-global `HOME`/`KIMI_CODE_HOME` are
//! never mutated in the test binary. Every case must keep the report native
//! (no Go route) and report exactly Go's `configured`/`auth_status`.
//! macOS only: the gate is removed only there (see `kimi_file_requires_go`).
//! Run as a non-root user; root bypasses the mode-000 cases.
#![cfg(target_os = "macos")]

use super::{kimi, needs_go_fallback};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

const ORACLE: &str = include_str!("../tests/fixtures/kimi_file_access_oracle.json");
const PROBE_MARKER: &str = "SYMBRAIN_KIMI_ACCESS_PROBE";

/// Child-process entry point; a no-op in the ordinary test run.
#[test]
fn kimi_access_probe_child() {
    if std::env::var_os(PROBE_MARKER).is_none() {
        return;
    }
    let provider = kimi();
    let observed = json!({
        "configured": provider.configured,
        "auth_status": provider.auth_status,
        "needs_go": needs_go_fallback(),
    });
    println!("PROBE {observed}");
}

fn apply(home: &Path, entries: &[Value]) {
    let expand = |value: &str| value.replace("$HOME", home.to_str().expect("UTF-8 root"));
    for entry in entries {
        let path = home.join(entry["path"].as_str().expect("entry path"));
        match entry["kind"].as_str().expect("entry kind") {
            "file" => {
                fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
                let mut contents = entry["contents"].as_str().unwrap_or_default().to_owned();
                let pad_to = usize::try_from(entry["pad_to"].as_u64().unwrap_or(0)).expect("size");
                if pad_to > contents.len() {
                    contents.push_str(&" ".repeat(pad_to - contents.len()));
                }
                fs::write(&path, contents).expect("credential file");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("file mode");
            }
            "dir" => fs::create_dir_all(&path).expect("directory"),
            "symlink" => {
                fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
                symlink(expand(entry["target"].as_str().expect("target")), &path).expect("symlink");
            }
            "chmod" => {
                let mode = u32::from_str_radix(entry["mode"].as_str().expect("mode"), 8)
                    .expect("octal mode");
                fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("chmod");
            }
            other => panic!("unknown entry kind {other}"),
        }
    }
}

/// Make every real directory traversable again so the temporary root can go.
fn restore_directory_modes(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.is_dir() {
        return;
    }
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    if let Ok(children) = fs::read_dir(path) {
        for child in children.flatten() {
            restore_directory_modes(&child.path());
        }
    }
}

fn probe(home: &Path, env: &serde_json::Map<String, Value>) -> Value {
    let name = format!(
        "{}::kimi_access_probe_child",
        module_path!()
            .strip_prefix("symbrain_usage::")
            .expect("crate-qualified module path")
    );
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env("HOME", home)
        .env(PROBE_MARKER, "1")
        // Short-circuits the automatic Claude Keychain listing: no host state.
        .env("ANTHROPIC_OAUTH_TOKEN", "synthetic-claude-oauth");
    for (name, value) in env {
        let value = value.as_str().expect("env value");
        command.env(
            name,
            value.replace("$HOME", home.to_str().expect("UTF-8 root")),
        );
    }
    let output = command.output().expect("probe child");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 probe output");
    assert!(output.status.success(), "probe failed: {stdout}");
    // The harness prefixes `test <name> ... ` to the first captured line.
    let start = stdout
        .find("PROBE ")
        .unwrap_or_else(|| panic!("no probe line in {stdout}"))
        + "PROBE ".len();
    let line = stdout[start..].lines().next().expect("probe line");
    serde_json::from_str(line).expect("probe JSON")
}

#[test]
fn kimi_cli_credential_file_shapes_match_go_and_stay_native() {
    let fixture: Value = serde_json::from_str(ORACLE).expect("Go Kimi file access oracle");
    let cases = fixture["cases"].as_array().expect("oracle cases");
    let (mut configured, mut missing) = (0, 0);
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let root = tempfile::tempdir().expect("temporary root");
        let home = root.path().join("home");
        fs::create_dir_all(&home).expect("home");
        apply(&home, case["entries"].as_array().expect("entries"));
        let empty = serde_json::Map::new();
        let env = case["env"].as_object().unwrap_or(&empty);
        let observed = probe(&home, env);
        restore_directory_modes(root.path());
        assert_eq!(
            observed["configured"], case["configured"],
            "{id}: configured"
        );
        assert_eq!(
            observed["auth_status"], case["auth_status"],
            "{id}: auth_status"
        );
        assert_eq!(observed["needs_go"], false, "{id}: must stay native");
        if case["configured"] == true {
            configured += 1;
        } else {
            missing += 1;
        }
    }
    // The control cases prove the probe can see a token; the rest are the
    // unsafe shapes Go reports as "no credentials".
    assert_eq!((cases.len(), configured, missing), (16, 6, 10));
}
