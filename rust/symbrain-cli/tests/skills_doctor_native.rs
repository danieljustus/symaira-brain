//! Native `symbrain skills doctor` default contract and fallback boundary.

#![cfg(unix)]
#![deny(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

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

fn fallback(root: &TempDir) -> PathBuf {
    let path = root.path().join("go-fallback");
    fs::write(
        &path,
        b"#!/bin/sh\nprintf 'fallback-stdout\\n'\nprintf 'fallback-stderr\\n' >&2\nexit 23\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn assert_fake_fallback(output: &Output) {
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(output.stdout, b"fallback-stdout\n");
    assert_eq!(output.stderr, b"fallback-stderr\n");
}

#[test]
fn default_doctor_matches_go_schema_and_formats() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let output = command(&root, &["skills", "doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
        .output()
        .unwrap();
    let project = root.path().join("project").canonicalize().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["config"]["Targets"], json!(null));
    assert_eq!(report["config"]["vcs"]["enabled"], json!(true));
    assert_eq!(
        report["config_path"],
        root.path()
            .join("config/symskills/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(
        report["log_path"],
        home.join(".local/share/symskills/events.jsonl")
            .display()
            .to_string()
    );
    assert_eq!(report["project_dir"], project.display().to_string());
    assert_eq!(report["targets"].as_array().unwrap().len(), 6);
    assert_eq!(
        report["targets"][3]["project"],
        project.join(".agents/skills").display().to_string()
    );
    assert!(!output.stdout[..output.stdout.len() - 1].contains(&b'\n'));

    let output = command(&root, &["skills", "doctor"]).output().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let table = String::from_utf8(output.stdout).unwrap();
    assert!(table.starts_with("config      "));
    assert!(table.contains("versioning  true\n"));
    assert!(table.ends_with("project     ") || table.contains("project     "));
}

#[test]
fn doctor_flags_and_dynamic_inputs_fallback_before_stdout() {
    for (name, args, env_name) in [
        (
            "target flag",
            &["skills", "doctor", "--target", "claude"][..],
            None,
        ),
        ("project config", &["skills", "doctor"][..], None),
        (
            "skills override",
            &["skills", "doctor"][..],
            Some("SYMSKILLS_LIBRARY_DIR"),
        ),
    ] {
        let root = TempDir::new().unwrap();
        if name == "project config" {
            fs::create_dir_all(root.path().join("project")).unwrap();
            fs::write(root.path().join("project/.symskills.toml"), b"vcs = {}\n").unwrap();
        }
        let mut command = command(&root, args);
        command.env("SYMBRAIN_GO_BINARY", fallback(&root));
        if let Some(env_name) = env_name {
            command.env(env_name, "/different/library");
        }
        assert_fake_fallback(&command.output().unwrap());
    }
}

#[test]
fn global_config_falls_back_before_stdout() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config/symskills");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), b"vcs = {}\n").unwrap();

    let output = command(&root, &["skills", "doctor"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}
