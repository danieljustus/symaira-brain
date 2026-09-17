//! Native `symbrain skills log` empty-state and Go-fallback byte contracts.

#![cfg(unix)]
#![deny(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

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
fn empty_log_is_native_with_exact_table_and_json_bytes() {
    for (args, expected) in [
        (
            &["skills", "log"][..],
            b"No recorded skill operations.\n"[..].to_vec(),
        ),
        (&["skills", "log", "--json"][..], b"[]\n"[..].to_vec()),
        (
            &["skills", "log", "--output", "json"][..],
            b"[]\n"[..].to_vec(),
        ),
        (
            &["skills", "--output", "json", "log"][..],
            b"[]\n"[..].to_vec(),
        ),
    ] {
        let root = TempDir::new().unwrap();
        let output = command(&root, args)
            .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    }
}

#[test]
fn existing_current_or_rotated_log_uses_go_before_stdout() {
    for file_name in ["events.jsonl", "events.1.jsonl"] {
        let root = TempDir::new().unwrap();
        let log_dir = root.path().join("home/.local/share/symskills");
        fs::create_dir_all(&log_dir).unwrap();
        fs::write(
            log_dir.join(file_name),
            br#"{"ts":"2026-09-17T00:00:00Z","event":"install","outcome":"ok","actor":"cli"}
"#,
        )
        .unwrap();
        let output = command(&root, &["skills", "log"])
            .env("SYMBRAIN_GO_BINARY", fallback(&root))
            .output()
            .unwrap();
        assert_fake_fallback(&output);
    }
}

#[test]
fn log_flags_filters_and_invalid_args_use_go_before_stdout() {
    for args in [
        &["skills", "log", "--skill", "demo"][..],
        &["skills", "log", "--target", "claude"][..],
        &["skills", "log", "--limit", "1"][..],
        &["skills", "log", "--bogus"][..],
    ] {
        let root = TempDir::new().unwrap();
        let output = command(&root, args)
            .env("SYMBRAIN_GO_BINARY", fallback(&root))
            .output()
            .unwrap();
        assert_fake_fallback(&output);
    }
}
