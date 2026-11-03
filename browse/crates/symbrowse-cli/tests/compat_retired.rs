//! The `compat` transport (Go/AzureTLS sidecar) is retired: selecting it from
//! TOML, environment or CLI flag returns one stable typed error, while the
//! static and browser selections keep working.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);
const MESSAGE: &str = "invalid configuration: transport_retired: the compat transport (Go/AzureTLS sidecar) has been retired and is no longer available; set mode to static or browser";
const SELECTION_MESSAGE: &str = "the compat transport (Go/AzureTLS sidecar) has been retired and is no longer available; set mode to static or browser";

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn root(name: &str) -> PathBuf {
    let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "symbrowse-compat-retired-{name}-{}-{id}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
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

fn assert_retired_json(output: &Output, message: &str) {
    assert_eq!(output.code, 9, "stderr={}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr={}", output.stderr);
    let value: Value = serde_json::from_str(&output.stdout).expect("JSON envelope");
    assert_eq!(value["success"], false);
    assert_eq!(value["error"]["code"], "config");
    assert_eq!(value["error"]["message"], message);
    assert_eq!(value["error"]["details"]["code"], "transport_retired");
    assert_eq!(value["error"]["hint"], "set mode to static or browser");
}

#[test]
fn config_show_rejects_compat_from_toml() {
    let root = root("toml");
    fs::write(
        root.join("xdg-config/symbrowse/config.toml"),
        "mode = \"compat\"\n",
    )
    .unwrap();
    assert_retired_json(&run(&root, &["config", "show", "--json"], &[]), MESSAGE);
    let text = run(&root, &["config", "show"], &[]);
    assert_eq!((text.code, text.stdout.as_str()), (9, ""));
    assert_eq!(text.stderr, format!("{MESSAGE}\n"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_show_rejects_compat_from_project_toml() {
    let root = root("project-toml");
    fs::write(root.join("work/.symbrowse.toml"), "mode = \"compat\"\n").unwrap();
    assert_retired_json(&run(&root, &["config", "show", "--json"], &[]), MESSAGE);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_show_rejects_compat_from_environment_and_flag() {
    let root = root("env-flag");
    assert_retired_json(
        &run(
            &root,
            &["config", "show", "--json"],
            &[("SYMBROWSE_MODE", "compat")],
        ),
        MESSAGE,
    );
    assert_retired_json(
        &run(
            &root,
            &["config", "show", "--json", "--mode", "compat"],
            &[],
        ),
        MESSAGE,
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn daemon_rejects_compat_before_binding_anything() {
    let root = root("daemon");
    let session = "retired-compat";
    let flag = run(
        &root,
        &["daemon", "--session", session, "--mode", "compat", "--json"],
        &[],
    );
    assert_retired_json(&flag, SELECTION_MESSAGE);
    let text = run(
        &root,
        &["daemon", "--session", session, "--mode", "compat"],
        &[],
    );
    assert_eq!((text.code, text.stdout.as_str()), (9, ""));
    assert_eq!(text.stderr, format!("{SELECTION_MESSAGE}\n"));
    let env = run(
        &root,
        &["daemon", "--session", session, "--json"],
        &[("SYMBROWSE_MODE", "compat")],
    );
    assert_retired_json(&env, MESSAGE);
    fs::write(
        root.join("xdg-config/symbrowse/config.toml"),
        "mode = \"compat\"\n",
    )
    .unwrap();
    assert_retired_json(
        &run(&root, &["daemon", "--session", session, "--json"], &[]),
        MESSAGE,
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mcp_server_rejects_compat_on_stderr_only() {
    let root = root("mcp");
    fs::write(
        root.join("xdg-config/symbrowse/config.toml"),
        "mode = \"compat\"\n",
    )
    .unwrap();
    let output = run(&root, &["mcp"], &[]);
    assert_eq!(output.code, 9);
    assert_eq!(output.stdout, "", "MCP stdout must stay clean");
    assert_eq!(output.stderr, format!("{MESSAGE}\n"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_and_browser_selection_are_unaffected() {
    let root = root("unaffected");
    for (mode, engine) in [
        ("static", "static"),
        ("browser", "chrome"),
        ("browser", "firefox"),
    ] {
        let output = run(
            &root,
            &["config", "show", "--json"],
            &[("SYMBROWSE_MODE", mode), ("SYMBROWSE_ENGINE", engine)],
        );
        assert_eq!(output.code, 0, "{mode}/{engine}: {}", output.stderr);
        let value: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(value["data"]["fields"]["engine"]["value"], engine);
    }
    let flag = run(
        &root,
        &[
            "config", "show", "--json", "--mode", "static", "--engine", "static",
        ],
        &[],
    );
    assert_eq!(flag.code, 0, "{}", flag.stderr);
    fs::remove_dir_all(root).unwrap();
}
