//! Shared helper for scenarios that used to be delegated to Go (#764).
//!
//! `expect` runs the native command and compares stdout, stderr and the exit
//! code with the bytes the real Go binary produced for the same scenario
//! (`fixtures/skills_native_fallback_oracle.json`). Setting
//! `SKILLS_NATIVE_RECORD_BIN` to a Go oracle build and
//! `SKILLS_NATIVE_RECORD_OUT` to a JSONL path re-records the entries: the Go
//! binary then runs first on the very same disposable root.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const FIXTURE: &str = include_str!("../fixtures/skills_native_fallback_oracle.json");

fn normalise(bytes: &[u8], root: &Path) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let raw = root.to_string_lossy().into_owned();
    if let Ok(canonical) = root.canonicalize() {
        text = text.replace(canonical.to_string_lossy().as_ref(), "$ROOT");
    }
    let text = text.replace(&raw, "$ROOT");
    let timestamp =
        regex::Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:\d\d)").unwrap();
    timestamp.replace_all(&text, "<TS>").into_owned()
}

fn go_twin(command: &Command, binary: &str) -> Command {
    let mut twin = Command::new(binary);
    twin.env_clear().args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => twin.env(key, value),
            None => twin.env_remove(key),
        };
    }
    if let Some(directory) = command.get_current_dir() {
        twin.current_dir(directory);
    }
    twin
}

fn record(label: &str, go: &Output, root: &Path) {
    let Ok(path) = std::env::var("SKILLS_NATIVE_RECORD_OUT") else {
        return;
    };
    let entry = serde_json::json!({
        "label": label,
        "code": go.status.code(),
        "out": normalise(&go.stdout, root),
        "err": normalise(&go.stderr, root),
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(format!("{entry}\n").as_bytes()).unwrap();
}

/// Runs `command` natively and requires the recorded Go bytes for `label`.
pub fn expect(label: &str, command: &mut Command, root: &Path) -> Output {
    let recording = std::env::var("SKILLS_NATIVE_RECORD_BIN").ok();
    if let Some(binary) = &recording {
        let go = go_twin(command, binary).output().unwrap();
        record(label, &go, root);
    }
    let native = command.output().unwrap();
    if recording.is_some() {
        return native;
    }
    let fixture: BTreeMap<String, Value> = serde_json::from_str(FIXTURE).unwrap();
    let want = fixture
        .get(label)
        .unwrap_or_else(|| panic!("no recorded Go bytes for {label}"));
    assert_eq!(
        native.status.code().map(i64::from),
        want["code"].as_i64(),
        "{label}: exit code"
    );
    assert_eq!(
        normalise(&native.stdout, root),
        want["out"].as_str().unwrap(),
        "{label}: stdout"
    );
    assert_eq!(
        normalise(&native.stderr, root),
        want["err"].as_str().unwrap(),
        "{label}: stderr"
    );
    native
}
