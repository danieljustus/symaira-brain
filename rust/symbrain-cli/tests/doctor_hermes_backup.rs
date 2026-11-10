//! #1024: doctor reads `$HERMES_HOME` and Hermes profile configs, and reports a
//! paused first-run backup (`.<app>-resume.partial`, #1020) with how to finish it.

#![deny(unsafe_code)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fs;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

fn doctor(root: &TempDir, args: &[&str], hermes_home: Option<&std::path::Path>) -> Command {
    let home = root.path().join("home");
    let empty_path = root.path().join("empty-path");
    for path in [&home, &empty_path, &root.path().join("project")] {
        fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", &empty_path)
        .env("LC_ALL", "C.UTF-8")
        .current_dir(root.path().join("project"))
        .arg("doctor")
        .args(args);
    if let Some(hermes_home) = hermes_home {
        command.env("HERMES_HOME", hermes_home);
    }
    command
}

fn report(root: &TempDir, hermes_home: Option<&std::path::Path>) -> Value {
    let output = doctor(root, &["--json"], hermes_home).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn hermes_home_and_profile_configs_are_checked_read_only() {
    let root = TempDir::new().unwrap();
    let hermes_home = root.path().join("alt-hermes");
    fs::create_dir_all(hermes_home.join("profiles/work")).unwrap();
    fs::create_dir_all(hermes_home.join("profiles/bad")).unwrap();
    fs::write(hermes_home.join("config.yaml"), "mcp_servers: {}\n").unwrap();
    fs::write(
        hermes_home.join("profiles/work/config.yaml"),
        "mcp_servers:\n  symbrain:\n    command: /opt/bin/symbrain\n    args: [mcp, -profile, ghost]\n",
    )
    .unwrap();
    fs::write(
        hermes_home.join("profiles/bad/config.yaml"),
        "mcp_servers: [\n",
    )
    .unwrap();
    let before = fs::read(hermes_home.join("profiles/work/config.yaml")).unwrap();

    let value = report(&root, Some(&hermes_home));
    let hermes = value["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "hermes")
        .unwrap();
    assert_eq!(
        hermes["config_path"],
        json!(hermes_home.join("config.yaml").to_str().unwrap())
    );
    assert_eq!(hermes["config_parsed"], json!(true));
    let profiles = value["hermes_profiles"].as_array().unwrap();
    let names: Vec<_> = profiles
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["hermes:bad", "hermes:work"]);
    assert_eq!(profiles[0]["config_parsed"], json!(false));
    assert_eq!(profiles[1]["installed"], json!(true));
    assert_eq!(profiles[1]["profile_missing"], json!(true));
    assert_eq!(profiles[1]["supports_mcp_install"], json!(false));

    let human = doctor(&root, &[], Some(&hermes_home)).output().unwrap();
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("\n  hermes profiles:\n    ✗  hermes:bad"),
        "{text}"
    );
    assert!(text.contains("hermes:work"), "{text}");
    assert!(
        text.contains("config found, symbrain not registered (read-only"),
        "{text}"
    );
    assert_eq!(
        fs::read(hermes_home.join("profiles/work/config.yaml")).unwrap(),
        before
    );
}

#[test]
fn without_hermes_profiles_or_pending_backup_the_report_is_unchanged() {
    let root = TempDir::new().unwrap();
    let value = report(&root, None);
    assert!(value.get("hermes_profiles").is_none());
    assert!(value.get("pending_backups").is_none());
    let human = String::from_utf8(doctor(&root, &[], None).output().unwrap().stdout).unwrap();
    assert!(!human.contains("hermes profiles:") && !human.contains("pending first-run backup"));
}

#[test]
fn paused_first_run_backup_is_reported_with_how_to_finish_it() {
    let root = TempDir::new().unwrap();
    let store = root
        .path()
        .join("home/.local/state/symaira/cutover-backups");
    let partial = store.join(".symbrain-resume.partial");
    fs::create_dir_all(&partial).unwrap();
    // A finished app (marker present) must not be reported.
    fs::create_dir_all(store.join(".symguard-resume.partial")).unwrap();
    fs::write(store.join("symguard.json"), "{}").unwrap();

    let value = report(&root, None);
    let pending = value["pending_backups"].as_array().unwrap();
    assert_eq!(pending.len(), 1, "{value}");
    assert_eq!(pending[0]["app"], json!("symbrain"));
    assert_eq!(pending[0]["partial_path"], json!(partial.to_str().unwrap()));
    assert_eq!(pending[0]["running"], json!(false));
    assert!(
        pending[0]["finish"]
            .as_str()
            .unwrap()
            .contains("symbrain version")
    );

    let human = String::from_utf8(doctor(&root, &[], None).output().unwrap().stdout).unwrap();
    assert!(
        human.contains("\n  pending first-run backup:\n    !  symbrain"),
        "{human}"
    );
    assert!(human.contains("(paused)\n       finish: "), "{human}");
    assert!(
        !store.join(".symbrain.lock").exists(),
        "doctor must not create backup state"
    );
}
