//! Declared CLI additions (`migration/cli-additions.md`): the reviewed list, no
//! shadowing of recorded commands, the printed usage, and real dispatch.

#[path = "common/cli_additions.rs"]
mod additions;

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Every command added after the Go recordings. Adding one means editing this
/// list too, so a reviewer sees each new command in the diff.
const DECLARED: [(&str, &str, &str); 3] = [
    (
        "browse",
        "Agent-operated browser via the built-in symbrowse worker",
        "any",
    ),
    (
        "operate",
        "GUI automation via the built-in symoperate worker",
        "macos",
    ),
    (
        "scope",
        "Local ports, containers and daemons via the built-in symscope worker",
        "macos",
    ),
];

#[derive(serde::Deserialize)]
struct Recorded {
    id: String,
    args: Vec<String>,
    stdout: String,
    stderr: String,
}

fn recorded_cases() -> Vec<Recorded> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli_tree_expectations.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("parse")
}

fn recorded(id: &str) -> Recorded {
    recorded_cases()
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("no recorded case {id:?}"))
}

/// Commands the Go binary knew: the names in its recorded usage plus the first
/// word of every recorded invocation it accepted (this includes the hidden
/// `serve`).
fn recorded_commands() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let usage = recorded("go help").stdout;
    let list = usage
        .split_once("\nCommands:\n")
        .and_then(|(_, rest)| rest.split_once("\nVault approval"))
        .expect("command list")
        .0;
    for line in list.lines().filter(|line| !line.is_empty()) {
        let name = line.trim_start().split(' ').next().expect("name");
        names.insert(name.to_owned());
    }
    for case in recorded_cases() {
        let accepted = !case.stderr.starts_with("symbrain: unknown command");
        if let Some(first) = case.args.first().filter(|_| accepted) {
            names.insert(first.clone());
        }
    }
    names
}

fn isolated(program: &Path, home: &Path, path_dir: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("PATH", path_dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run symbrain")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let root = tempfile::tempdir().expect("temp dir");
    let home = root.path().canonicalize().expect("canonical root");
    let output = isolated(
        Path::new(env!("CARGO_BIN_EXE_symbrain")),
        &home,
        &home,
        args,
    );
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        String::from_utf8(output.stderr).expect("utf-8 stderr"),
    )
}

#[test]
fn the_declared_additions_are_the_reviewed_list() {
    let declared: Vec<(String, String, String)> = additions::declared()
        .into_iter()
        .map(|a| (a.name, a.short, a.platform))
        .collect();
    let reviewed: Vec<(String, String, String)> = DECLARED
        .iter()
        .map(|(name, short, platform)| {
            (
                (*name).to_owned(),
                (*short).to_owned(),
                (*platform).to_owned(),
            )
        })
        .collect();
    assert_eq!(declared, reviewed);
}

#[test]
fn no_addition_shadows_a_recorded_command() {
    let recorded = recorded_commands();
    for expected in ["init", "guard", "version", "help", "serve", "vault"] {
        assert!(
            recorded.contains(expected),
            "{expected} not found: {recorded:?}"
        );
    }
    for addition in additions::declared() {
        assert!(
            !recorded.contains(&addition.name),
            "{} is a recorded command",
            addition.name
        );
    }
}

#[test]
fn every_usage_route_prints_the_recording_plus_the_declared_lines() {
    let help = recorded("go help").stdout;
    let expected = additions::with_additions(&help);
    assert_ne!(expected, help);
    for args in [&["help"][..], &["--help"], &["-h"]] {
        let (code, stdout, stderr) = run(args);
        assert_eq!((code, stderr.as_str()), (0, ""), "{args:?}");
        assert_eq!(stdout, expected, "{args:?}");
    }
    let (code, stdout, _) = run(&[]);
    assert_eq!((code, stdout.as_str()), (2, expected.as_str()));
    let (code, stdout, stderr) = run(&["bogus"]);
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert_eq!(
        stderr,
        additions::with_additions(&recorded("go bogus").stderr)
    );
    for addition in additions::declared() {
        assert!(
            expected.contains(&additions::line(&addition)),
            "{}",
            addition.name
        );
    }
}

/// A copy of the binary in an empty directory, so no real worker sits beside it.
fn isolated_copy(base: &Path) -> PathBuf {
    let exe_dir = base.join("exe");
    std::fs::create_dir_all(&exe_dir).unwrap();
    let exe = exe_dir.join("symbrain");
    std::fs::copy(env!("CARGO_BIN_EXE_symbrain"), &exe).unwrap();
    exe
}

#[test]
fn every_declared_command_is_dispatched_not_just_listed() {
    let root = tempfile::tempdir().expect("temp dir");
    let base = root.path().canonicalize().expect("canonical root");
    let exe = isolated_copy(&base);
    for addition in additions::declared() {
        let output = isolated(&exe, &base, &base, &[addition.name.as_str(), "--help"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.starts_with("symbrain: unknown command"),
            "{} is listed but not dispatched: {stderr}",
            addition.name
        );
    }
}

/// The binary hands a declared worker command to its worker, or says so when
/// the platform has none. A copy of the binary runs next to no real worker and
/// a fake worker is the only one on `PATH`.
#[cfg(unix)]
#[test]
fn declared_worker_commands_reach_their_worker() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("temp dir");
    let base = root.path().canonicalize().expect("canonical root");
    let exe = isolated_copy(&base);
    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    for addition in additions::declared() {
        let worker = bin_dir.join(format!("sym{}", addition.name));
        std::fs::write(&worker, "#!/bin/sh\necho \"fake-worker $0 $*\"\nexit 7\n").unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for addition in additions::declared() {
        let output = isolated(
            &exe,
            &base,
            &bin_dir,
            &[addition.name.as_str(), "probe", "--flag"],
        );
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if addition.platform == "macos" && !cfg!(target_os = "macos") {
            assert_eq!(output.status.code(), Some(1), "{}: {stderr}", addition.name);
            assert!(stderr.contains("unavailable on this platform"), "{stderr}");
            assert!(stdout.is_empty(), "{stdout}");
        } else {
            assert_eq!(output.status.code(), Some(7), "{}: {stderr}", addition.name);
            assert!(
                stdout.contains(&format!("sym{} probe --flag", addition.name)),
                "{stdout}"
            );
        }
    }
}
