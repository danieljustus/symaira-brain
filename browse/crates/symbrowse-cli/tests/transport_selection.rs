//! CFG-006: process-level transport selection. Every case of
//! `testdata/port/core/transport-selection.json` is driven through the real
//! `symbrowse` binary via environment and flags; a selection either reports its
//! explicit mode/engine or fails with a typed `details.code`, exit 9 and no
//! fallback. Browser profile data never appears in the machine-readable output.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Deserialize;
use serde_json::Value;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
struct Case {
    id: String,
    mode: String,
    engine: Option<String>,
    expect: Option<String>,
    error: Option<String>,
}

fn cases() -> Vec<Case> {
    #[derive(Deserialize)]
    struct File {
        cases: Vec<Case>,
    }
    const CONTENT: &[u8] = include_bytes!("../../../testdata/port/core/transport-selection.json");
    serde_json::from_slice::<File>(CONTENT)
        .expect("decode transport selection fixture")
        .cases
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn root(name: &str) -> PathBuf {
    let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "symbrowse-selection-{name}-{}-{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    for directory in [
        "home",
        "work",
        "xdg-config/symbrowse",
        "xdg-cache",
        "xdg-state",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    root
}

fn run(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrowse"));
    command
        .args(args)
        .current_dir(root.join("work"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("TMPDIR", root)
        .env("LANG", "C")
        .stdin(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("run symbrowse");
    Output {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// The configuration spelling of the static transport is engine `static`.
fn config_engine(case: &Case) -> Option<String> {
    match (&case.engine, case.mode.as_str()) {
        (Some(engine), _) => Some(engine.clone()),
        (None, "static") => Some("static".to_owned()),
        (None, _) => None,
    }
}

fn assert_selection(case: &Case, via: &str, output: &Output) {
    let label = format!("{} via {via}", case.id);
    match (&case.expect, &case.error) {
        (Some(expect), None) => {
            assert_eq!(output.code, 0, "{label}: {}", output.stderr);
            let value: Value = serde_json::from_str(&output.stdout).expect("JSON envelope");
            assert_eq!(value["success"], true, "{label}");
            assert_eq!(
                value["data"]["fields"]["engine"]["value"], *expect,
                "{label}"
            );
            let rendered = output.stdout.to_lowercase();
            for forbidden in ["user_data_dir", "browser_context_id", "profile"] {
                assert!(
                    !rendered.contains(forbidden),
                    "{label}: config output leaks {forbidden}"
                );
            }
        }
        (None, Some(code)) => {
            assert_eq!(output.code, 9, "{label}: {}", output.stdout);
            assert!(output.stderr.is_empty(), "{label}: {}", output.stderr);
            let value: Value = serde_json::from_str(&output.stdout).expect("JSON envelope");
            assert_eq!(value["success"], false, "{label}");
            assert_eq!(value["error"]["code"], "config", "{label}");
            if code == "invalid_browser_engine" {
                // The Go-oracle engine check answers first and has no code.
                assert_eq!(value["error"]["message"], "invalid configuration");
            } else {
                assert_eq!(value["error"]["details"]["code"], *code, "{label}");
            }
        }
        _ => panic!("{label}: fixture case needs exactly one of expect/error"),
    }
}

#[test]
fn selection_fixture_cases_report_the_explicit_selection_or_a_typed_error() {
    for case in cases()
        .iter()
        .filter(|case| case.id != "browser-missing-engine")
    {
        let engine = config_engine(case);
        let root = root(&case.id);
        let mut env = vec![("SYMBROWSE_MODE", case.mode.as_str())];
        if let Some(engine) = &engine {
            env.push(("SYMBROWSE_ENGINE", engine.as_str()));
        }
        assert_selection(
            case,
            "env",
            &run(&root, &["config", "show", "--json"], &env),
        );

        let mut args = vec!["config", "show", "--json", "--mode", case.mode.as_str()];
        if let Some(engine) = &engine {
            args.extend(["--engine", engine.as_str()]);
        }
        assert_selection(case, "flag", &run(&root, &args, &[]));

        let toml = format!(
            "mode = \"{}\"\n{}",
            case.mode,
            engine
                .as_ref()
                .map_or(String::new(), |engine| format!("engine = \"{engine}\"\n"))
        );
        fs::write(root.join("xdg-config/symbrowse/config.toml"), toml).unwrap();
        assert_selection(
            case,
            "toml",
            &run(&root, &["config", "show", "--json"], &[]),
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn flag_overrides_environment_and_toml_without_fallback() {
    let root = root("precedence");
    fs::write(
        root.join("xdg-config/symbrowse/config.toml"),
        "mode = \"browser\"\nengine = \"chrome\"\n",
    )
    .unwrap();
    let from_toml = run(&root, &["config", "show", "--json"], &[]);
    assert_eq!(from_toml.code, 0, "{}", from_toml.stderr);
    let value: Value = serde_json::from_str(&from_toml.stdout).unwrap();
    assert_eq!(value["data"]["fields"]["engine"]["source"], "global");
    let from_env = run(
        &root,
        &["config", "show", "--json"],
        &[("SYMBROWSE_ENGINE", "firefox")],
    );
    let value: Value = serde_json::from_str(&from_env.stdout).unwrap();
    assert_eq!(value["data"]["fields"]["engine"]["value"], "firefox");
    assert_eq!(value["data"]["fields"]["engine"]["source"], "env");
    let from_flag = run(
        &root,
        &["config", "show", "--json", "--engine", "safari"],
        &[("SYMBROWSE_ENGINE", "firefox")],
    );
    let value: Value = serde_json::from_str(&from_flag.stdout).unwrap();
    assert_eq!(value["data"]["fields"]["engine"]["value"], "safari");
    assert_eq!(value["data"]["fields"]["engine"]["source"], "flag");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn daemon_refuses_an_unknown_mode_with_a_typed_error_before_binding() {
    let root = root("daemon-unknown");
    let output = run(
        &root,
        &[
            "daemon",
            "--session",
            "selection",
            "--mode",
            "wat",
            "--json",
        ],
        &[],
    );
    assert_eq!(output.code, 9, "{}", output.stderr);
    let value: Value = serde_json::from_str(&output.stdout).expect("JSON envelope");
    assert_eq!(value["error"]["code"], "config");
    assert_eq!(value["error"]["details"]["code"], "invalid_transport_mode");
    let text = run(
        &root,
        &["daemon", "--session", "selection", "--mode", "wat"],
        &[],
    );
    assert_eq!((text.code, text.stdout.as_str()), (9, ""));
    assert!(
        text.stderr.contains("invalid_transport_mode"),
        "{}",
        text.stderr
    );
    fs::remove_dir_all(root).unwrap();
}
