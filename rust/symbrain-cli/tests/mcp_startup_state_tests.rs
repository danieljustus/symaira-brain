//! #834: disabled Memory/Audit cores create no startup state; history is opt-in.
//!
//! Every case runs the real `symbrain mcp` binary in a disposable HOME/XDG
//! tree and snapshots the whole tree afterwards.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

const INPUT: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"patterns"}}"#,
    "\n",
);

struct Env {
    root: TempDir,
}

impl Env {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        for dir in ["home", "config", "state", "cache", "runtime", "profiles"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn profile(&self, body: &str) -> PathBuf {
        let path = self.path("profiles/p.toml");
        std::fs::write(&path, format!("[profile]\nname = \"p\"\n\n{body}")).unwrap();
        path
    }

    fn global_config(&self, body: &str) {
        let dir = self.path("config/symbrain");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), body).unwrap();
    }

    fn run(&self, profile: &Path, extra_env: &[(&str, &str)]) -> Output {
        let root = self.root.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .env_clear()
            .envs(coverage::profile_environment())
            .args(["mcp", "--profile-file", profile.to_str().unwrap()])
            .env("HOME", root.join("home"))
            .env("USERPROFILE", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("home/.local/share"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("PATH", root.join("empty-path"))
            .env("TMPDIR", root)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(INPUT.as_bytes())
            .unwrap();
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }

    /// Relative paths of everything under the state roots, excluding only the
    /// test's own inputs (`profiles/` and the written global config).
    fn created(&self) -> Vec<String> {
        let mut found = Vec::new();
        for dir in ["home", "config", "state", "cache", "runtime"] {
            walk(self.root.path(), &self.path(dir), &mut found);
        }
        found.retain(|entry| entry != "config/symbrain" && entry != "config/symbrain/config.toml");
        found.sort();
        found
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        out.push(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        if path.is_dir() {
            walk(root, &path, out);
        }
    }
}

fn assert_served(output: &Output) {
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"id\":3"),
        "stdout: {:?}",
        output.stdout
    );
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
}

const DISABLED_CORES: &str = "[servers.memory]\nenabled = false\n\n[audit]\nenabled = false\n";

#[test]
fn disabled_memory_and_audit_create_no_startup_state_and_no_history() {
    let env = Env::new();
    let profile = env.profile(DISABLED_CORES);
    let output = env.run(&profile, &[]);
    assert_served(&output);
    assert_eq!(env.created(), Vec::<String>::new());
}

#[test]
fn profile_opt_out_is_authoritative_over_default_enabled_global_audit() {
    let env = Env::new();
    env.global_config("[audit]\nenabled = true\n");
    let profile = env.profile(DISABLED_CORES);
    assert_served(&env.run(&profile, &[("SYMBRAIN_AUDIT_ENABLED", "true")]));
    assert_eq!(env.created(), Vec::<String>::new());
}

#[test]
fn global_audit_opt_out_vetoes_profile_opt_in() {
    for (config, vars) in [
        ("[audit]\nenabled = false\n", Vec::new()),
        ("", vec![("SYMBRAIN_AUDIT_ENABLED", "false")]),
        (
            "[audit]\nenabled = true\n",
            vec![("SYMBRAIN_AUDIT_ENABLED", "0")],
        ),
    ] {
        let env = Env::new();
        env.global_config(config);
        let profile = env.profile("[audit]\nenabled = true\n");
        assert_served(&env.run(&profile, &vars));
        assert_eq!(env.created(), Vec::<String>::new(), "{config:?} {vars:?}");
    }
}

#[test]
fn audit_is_created_only_when_profile_and_global_setting_both_enable_it() {
    for config in ["", "[audit]\nenabled = true\n", "audit = \"ignored\"\n"] {
        let env = Env::new();
        env.global_config(config);
        let profile = env.profile("[audit]\nenabled = true\n");
        assert_served(&env.run(&profile, &[]));
        let created = env.created();
        assert!(
            created.contains(&"home/.local/share/symbrain/audit/p.jsonl".to_string()),
            "{config:?}: {created:?}"
        );
        assert!(
            !created.iter().any(|entry| entry.contains("patterns")),
            "history must stay opt-in: {created:?}"
        );
    }
}

#[test]
fn enabled_memory_is_the_positive_control_and_history_stays_absent() {
    let env = Env::new();
    let profile = env.profile(
        "[servers.memory]\nenabled = true\nmode = \"read_write\"\n\n[audit]\nenabled = false\n",
    );
    assert_served(&env.run(&profile, &[]));
    let created = env.created();
    assert!(
        created.iter().any(|entry| entry.ends_with("default.db")),
        "enabled memory should open its store: {created:?}"
    );
    assert!(
        !created
            .iter()
            .any(|entry| entry.contains("patterns") || entry.contains("audit")),
        "{created:?}"
    );
}

#[test]
fn invalid_global_audit_value_fails_before_startup_even_when_disabled() {
    for (config, vars, message) in [
        (
            "[audit]\nenabled = 3\n",
            Vec::new(),
            "config: audit.enabled must be a boolean",
        ),
        ("[audit\n", Vec::new(), "config: failed to parse"),
        (
            "",
            vec![("SYMBRAIN_AUDIT_ENABLED", "maybe")],
            "config: SYMBRAIN_AUDIT_ENABLED must be a boolean",
        ),
    ] {
        let env = Env::new();
        env.global_config(config);
        let profile = env.profile(DISABLED_CORES);
        let output = env.run(&profile, &vars);
        assert_eq!(output.status.code(), Some(2), "{config:?} {vars:?}");
        assert!(output.stdout.is_empty(), "{:?}", output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{config:?}: {stderr}");
        assert_eq!(env.created(), Vec::<String>::new());
    }
}
