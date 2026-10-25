//! Native `symbrain skills doctor` default contract and fallback boundary.

#![cfg(unix)]
#![deny(unsafe_code)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

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
fn flags_and_symskills_env_are_native_like_go() {
    // Go ignores `--target`/`--scope` for doctor and applies the SYMSKILLS_*
    // environment verbatim (differential: `skills_status_config_doctor_*`).
    let root = TempDir::new().unwrap();
    let missing_go = root.path().join("missing-go");
    let default = command(&root, &["skills", "doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", &missing_go)
        .output()
        .unwrap();
    let flagged = command(&root, &["skills", "doctor", "--target", "claude", "--json"])
        .env("SYMBRAIN_GO_BINARY", &missing_go)
        .output()
        .unwrap();
    assert!(flagged.status.success(), "stderr: {:?}", flagged.stderr);
    assert_eq!(flagged.stdout, default.stdout);

    let overridden = command(&root, &["skills", "doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", &missing_go)
        .env("SYMSKILLS_LIBRARY_DIR", "/different/library")
        .env("SYMSKILLS_PROFILES_DIR", "")
        .env("SYMSKILLS_VCS_ENABLED", "F")
        .output()
        .unwrap();
    assert!(
        overridden.status.success(),
        "stderr: {:?}",
        overridden.stderr
    );
    assert!(overridden.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&overridden.stdout).unwrap();
    assert_eq!(report["config"]["library_dir"], "/different/library");
    assert_eq!(report["config"]["profiles_dir"], "");
    assert_eq!(report["profiles_dir"], "");
    assert_eq!(report["config"]["vcs"]["enabled"], json!(false));

    let table = command(&root, &["skills", "doctor"])
        .env("SYMBRAIN_GO_BINARY", &missing_go)
        .env("SYMSKILLS_PROFILES_DIR", "")
        .env("SYMSKILLS_BASE_DIR", "  ")
        .output()
        .unwrap();
    let table = String::from_utf8(table.stdout).unwrap();
    assert!(table.contains("profiles    -\n"), "{table}");
    assert!(table.contains("base        -\n"), "{table}");
}

#[test]
fn failing_config_probe_resets_environment_to_defaults() {
    // A directory at config.toml makes Go's `config.Load()` fail, and the
    // whole config — the environment included — falls back to the defaults.
    let root = TempDir::new().unwrap();
    let missing_go = root.path().join("missing-go");
    let default = command(&root, &["skills", "doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", &missing_go)
        .output()
        .unwrap();
    fs::create_dir_all(root.path().join("config/symskills/config.toml")).unwrap();
    for (name, value) in [
        ("SYMSKILLS_LIBRARY_DIR", "/different/library"),
        ("SYMSKILLS_VCS_ENABLED", "maybe"),
    ] {
        let output = command(&root, &["skills", "doctor", "--json"])
            .env("SYMBRAIN_GO_BINARY", &missing_go)
            .env(name, value)
            .output()
            .unwrap();
        assert!(output.status.success(), "{name}: {:?}", output.stderr);
        assert_eq!(output.stdout, default.stdout, "{name}");
    }
}

#[test]
fn undecoded_shapes_fall_back_before_stdout() {
    for (name, args, env) in [
        (
            "flag error",
            &["skills", "doctor", "--bogus"][..],
            None::<(&str, &str)>,
        ),
        (
            "positional argument",
            &["skills", "doctor", "extra"][..],
            None,
        ),
        (
            "project config the decoder hands to Go",
            &["skills", "doctor"][..],
            None,
        ),
        (
            "unclean xdg base",
            &["skills", "doctor"][..],
            Some(("XDG_DATA_HOME", "/data/../data")),
        ),
        (
            "line separator in override",
            &["skills", "doctor"][..],
            Some(("SYMSKILLS_LIBRARY_DIR", "/a\u{2028}b")),
        ),
    ] {
        let root = TempDir::new().unwrap();
        if name == "project config the decoder hands to Go" {
            fs::create_dir_all(root.path().join("project")).unwrap();
            fs::write(root.path().join("project/.symskills.toml"), b"[unclosed\n").unwrap();
        }
        let mut command = command(&root, args);
        command.env("SYMBRAIN_GO_BINARY", fallback(&root));
        if let Some((name, value)) = env {
            command.env(name, value);
        }
        assert_fake_fallback(&command.output().unwrap());
    }
}

#[test]
fn global_config_the_decoder_hands_to_go_falls_back_before_stdout() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config/symskills");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), b"library_dir = \n").unwrap();

    let output = command(&root, &["skills", "doctor"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}

#[test]
fn config_files_are_decoded_natively_with_go_precedence() {
    // SKL-019: valid TOML is decoded by the Go-exact SKL-018 decoder, the
    // project file overrides the global one, the environment overrides both and
    // an apply error resets the whole config to the defaults.
    let root = TempDir::new().unwrap();
    let missing_go = root.path().join("missing-go");
    let report = |root: &TempDir, env: &[(&str, &str)]| -> serde_json::Value {
        let mut command = command(root, &["skills", "doctor", "--json"]);
        command.env("SYMBRAIN_GO_BINARY", &missing_go);
        for (name, value) in env {
            command.env(name, value);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let default = report(&root, &[]);
    let config = root.path().join("config/symskills");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(root.path().join("project")).unwrap();
    fs::write(
        config.join("config.toml"),
        "\u{feff}library_dir = \"/global/library\"\ncache_dir = \"/global/cache\"\nvcs = { enabled = \"false\" }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("project/.symskills.toml"),
        "library_dir = \"/project/library\"\n",
    )
    .unwrap();
    let merged = report(&root, &[]);
    assert_eq!(merged["config"]["library_dir"], "/project/library");
    assert_eq!(merged["config"]["cache_dir"], "/global/cache");
    assert_eq!(merged["config"]["vcs"]["enabled"], json!(false));
    assert_eq!(
        merged["config"]["base_dir"], default["config"]["base_dir"],
        "unset keys keep the defaults"
    );

    let env = report(
        &root,
        &[
            ("SYMSKILLS_LIBRARY_DIR", "/env/library"),
            ("SYMSKILLS_VCS_ENABLED", "T"),
        ],
    );
    assert_eq!(env["config"]["library_dir"], "/env/library");
    assert_eq!(env["config"]["cache_dir"], "/global/cache");
    assert_eq!(env["config"]["vcs"]["enabled"], json!(true));

    // A wrong type is a Go apply error: the whole config, environment
    // included, falls back to the defaults.
    fs::write(config.join("config.toml"), "library_dir = 5\n").unwrap();
    let reset = report(&root, &[("SYMSKILLS_BASE_DIR", "/env/base")]);
    assert_eq!(reset["config"], default["config"]);
}
