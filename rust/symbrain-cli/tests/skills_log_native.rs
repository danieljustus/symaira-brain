//! Native `symbrain skills` empty-log and Go-fallback byte contracts.

#![cfg(unix)]
#![deny(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    let cache = root.join("cache");
    let project = root.join("project");
    let empty_path = root.join("empty-path");
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
fn readable_current_and_rotated_logs_are_native_newest_first() {
    let root = TempDir::new().unwrap();
    let log_dir = root.path().join("home/.local/share/symskills");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("events.1.jsonl"),
        br#"{"ts":"2026-09-17T00:00:00Z","event":"install","skill":"old","target":"claude","outcome":"ok","actor":"cli"}
corrupt
"#,
    )
    .unwrap();
    fs::write(
        log_dir.join("events.jsonl"),
        br#"{"ts":"2026-09-17T01:00:00Z","event":"render","skill":"new","target":"opencode","outcome":"ok","actor":"cli"}
{"ts":"2026-09-17T02:00:00Z","event":"install","skill":"latest<&>","target":"opencode","outcome":"error","actor":"cli"}
"#,
    )
    .unwrap();

    let output = command(&root, &["skills", "log", "--json"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let records: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["skill"], "latest<&>");
    assert_eq!(records[1]["skill"], "new");
    assert_eq!(records[2]["skill"], "old");
    assert!(records[0].get("tool_version").is_none());
    assert!(
        output
            .stdout
            .windows(b"latest\\u003c\\u0026\\u003e".len())
            .any(|window| window == b"latest\\u003c\\u0026\\u003e"),
        "JSON must retain Go's HTML escaping: {:?}",
        output.stdout
    );
}

#[test]
fn unavailable_log_path_uses_go_before_stdout() {
    let root = TempDir::new().unwrap();
    let share_dir = root.path().join("home/.local/share");
    fs::create_dir_all(&share_dir).unwrap();
    fs::write(share_dir.join("symskills"), b"not a directory").unwrap();

    let output = command(&root, &["skills", "log"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}

#[test]
fn doctor_uses_go_before_stdout() {
    let root = TempDir::new().unwrap();
    let output = command(&root, &["skills", "doctor", "--json"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}

#[test]
fn log_flags_filter_and_limit_natively_but_invalid_args_use_go() {
    for args in [
        &["skills", "log", "--skill", "demo"][..],
        &["skills", "log", "--target", "claude"][..],
        &["skills", "log", "--limit", "1"][..],
    ] {
        let root = TempDir::new().unwrap();
        let log_dir = root.path().join("home/.local/share/symskills");
        fs::create_dir_all(&log_dir).unwrap();
        fs::write(
            log_dir.join("events.jsonl"),
            br#"{"ts":"2026-09-17T00:00:00Z","event":"install","skill":"demo","target":"claude","outcome":"ok","actor":"cli"}
{"ts":"2026-09-17T01:00:00Z","event":"render","skill":"other","target":"opencode","outcome":"ok","actor":"cli"}
"#,
        )
        .unwrap();
        let output = command(&root, args)
            .env("SYMBRAIN_GO_BINARY", fallback(&root))
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
    }

    let root = TempDir::new().unwrap();
    let output = command(&root, &["skills", "log", "--bogus"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}

#[test]
fn log_filters_trim_values_and_ignore_blank_filters() {
    let root = TempDir::new().unwrap();
    let log_dir = root.path().join("home/.local/share/symskills");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("events.jsonl"),
        br#"{"ts":"2026-09-17T00:00:00Z","event":"install","skill":"demo","target":"claude","outcome":"ok","actor":"cli"}
{"ts":"2026-09-17T01:00:00Z","event":"render","skill":"other","target":"opencode","outcome":"ok","actor":"cli"}
"#,
    )
    .unwrap();

    for args in [
        &["skills", "log", "--json", "--skill", " demo "][..],
        &["skills", "log", "--json", "--skill", "   "][..],
        &["skills", "log", "--json", "--target", ""][..],
    ] {
        let output = command(&root, args)
            .env("SYMBRAIN_GO_BINARY", root.path().join("missing-go"))
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        let records: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(records.len(), if args[4].trim().is_empty() { 2 } else { 1 });
    }
}

#[test]
fn symlinked_log_uses_go_before_stdout() {
    let root = TempDir::new().unwrap();
    let log_dir = root.path().join("home/.local/share/symskills");
    let target = root.path().join("outside-events.jsonl");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(&target, b"{}\n").unwrap();
    std::os::unix::fs::symlink(&target, log_dir.join("events.jsonl")).unwrap();

    let output = command(&root, &["skills", "log"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}

#[test]
fn symlinked_log_ancestor_uses_go_before_stdout() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let real_local = home.join("real-local");
    let log_dir = real_local.join("share/symskills");
    fs::create_dir_all(&log_dir).unwrap();
    std::os::unix::fs::symlink(&real_local, home.join(".local")).unwrap();
    fs::write(
        log_dir.join("events.jsonl"),
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

#[test]
fn unreadable_log_uses_go_before_stdout() {
    let root = TempDir::new().unwrap();
    let log_dir = root.path().join("home/.local/share/symskills");
    fs::create_dir_all(&log_dir).unwrap();
    let path = log_dir.join("events.jsonl");
    fs::write(&path, b"{}\n").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o000);
    fs::set_permissions(&path, permissions).unwrap();

    let output = command(&root, &["skills", "log"])
        .env("SYMBRAIN_GO_BINARY", fallback(&root))
        .output()
        .unwrap();
    assert_fake_fallback(&output);
}
