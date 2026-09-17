#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn setup_fix_uses_native_noop_when_all_managed_versions_match() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let bin_dir = home.join(".symaira/bin");
    fs::create_dir_all(&bin_dir).unwrap();

    for (name, version) in [
        ("symcockpit", "0.6.2"),
        ("symdesk", "0.12.2"),
        ("symvault", "0.22.1"),
    ] {
        let path = bin_dir.join(name);
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf '{{\"version\":\"{version}\"}}\\n'\n"),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let fallback = root.path().join("fallback");
    fs::write(&fallback, b"#!/bin/sh\nprintf fallback >&2\nexit 17\n").unwrap();
    fs::set_permissions(&fallback, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
        .args(["setup", "--fix", "--json"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", "/usr/bin:/bin")
        .env("SYMBRAIN_GO_BINARY", &fallback)
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["results"].as_array().unwrap().len(), 3);
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| result["status"] == "skipped")
    );
}
