//! `symbrain mcp` starts the bundled `symoperate` / `symscope` workers as
//! gateway backends exactly when a profile enables them (#1092).
//!
//! The workers are fake scripts that speak minimal MCP over stdio and log each
//! invocation. They sit on `PATH`, the same lookup the real workers use after
//! the directory of the running executable.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fmt::Write as _;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

/// `{tool}` and `{log}` are substituted per worker.
const FAKE_WORKER: &str = r#"#!/usr/bin/python3
import json
import sys

with open("{log}", "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\n")

for line in sys.stdin:
    try:
        request = json.loads(line)
    except Exception:
        continue
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [
            {"name": "{tool}", "description": "fake", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": True}},
            {"name": "{tool}_hidden", "description": "fake", "inputSchema": {"type": "object"}},
        ]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "called"}], "isError": False}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Method not found"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
"#;

struct Worker {
    alias: &'static str,
    binary: &'static str,
    tool: &'static str,
}

const OPERATE: Worker = Worker {
    alias: "operate",
    binary: "symoperate",
    tool: "version",
};
const SCOPE: Worker = Worker {
    alias: "scope",
    binary: "symscope",
    tool: "ports_list",
};

struct Setup {
    root: TempDir,
}

impl Setup {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("bin")).unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        Self { root }
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    fn log(&self, worker: &Worker) -> PathBuf {
        self.root.path().join(format!("{}.log", worker.binary))
    }

    fn install(&self, worker: &Worker) {
        let script = FAKE_WORKER
            .replace("{log}", &self.log(worker).to_string_lossy())
            .replace("{tool}", worker.tool);
        let path = self.bin().join(worker.binary);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn invocations(&self, worker: &Worker) -> Vec<Vec<String>> {
        match std::fs::read_to_string(self.log(worker)) {
            Ok(text) => text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("read worker log: {error}"),
        }
    }

    fn profile(&self, enabled: &[&Worker]) -> PathBuf {
        let mut text = String::from("[profile]\nname = \"workers\"\n");
        for worker in enabled {
            let _ = write!(
                text,
                "\n[servers.{}]\nenabled = true\ntools_allow = [\"{}\"]\n",
                worker.alias, worker.tool
            );
        }
        let path = self.root.path().join("workers.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Runs the real gateway binary: initialize, tools/list, then EOF.
    fn run(&self, profile: &Path) -> Output {
        let root = self.root.path();
        let mut child = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .envs(coverage::profile_environment())
            .args(["mcp", "--profile-file", &profile.to_string_lossy()])
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("home/.local/share"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("PATH", self.bin())
            .env("TMPDIR", root)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }
}

fn tool_names(output: &Output) -> Vec<String> {
    let responses: Vec<Value> = String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("stdout is pure JSON-RPC"))
        .collect();
    assert_eq!(responses.len(), 2, "{responses:?}");
    responses[1]["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

fn enabled_alias_starts_the_worker_and_exposes_its_allowlisted_tools(worker: &Worker) {
    let setup = Setup::new();
    setup.install(worker);
    let output = setup.run(&setup.profile(&[worker]));

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names = tool_names(&output);
    assert!(names.contains(&worker.tool.to_string()), "{names:?}");
    assert!(
        !names.contains(&format!("{}_hidden", worker.tool)),
        "tools outside the profile allowlist leaked: {names:?}"
    );
    assert_eq!(
        setup.invocations(worker),
        [["serve"]],
        "{} must start with its MCP entry command",
        worker.binary
    );
}

fn alias_not_enabled_never_starts_the_worker(worker: &Worker) {
    let setup = Setup::new();
    setup.install(worker);
    let profile = setup.root.path().join("plain.toml");
    std::fs::write(&profile, "[profile]\nname = \"plain\"\n").unwrap();
    let output = setup.run(&profile);

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert!(!tool_names(&output).contains(&worker.tool.to_string()));
    assert!(
        setup.invocations(worker).is_empty(),
        "{} was started although {} is not enabled",
        worker.binary,
        worker.alias
    );

    let explicitly_off = setup.root.path().join("off.toml");
    std::fs::write(
        &explicitly_off,
        format!(
            "[profile]\nname = \"off\"\n\n[servers.{}]\nenabled = false\ntools_allow = [\"{}\"]\n",
            worker.alias, worker.tool
        ),
    )
    .unwrap();
    let output = setup.run(&explicitly_off);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!tool_names(&output).contains(&worker.tool.to_string()));
    assert!(setup.invocations(worker).is_empty());
}

fn missing_worker_degrades_with_one_diagnostic(worker: &Worker) {
    let setup = Setup::new();
    let output = setup.run(&setup.profile(&[worker]));

    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.starts_with(&format!("symbrain mcp: {}: ", worker.alias)),
        "{stderr}"
    );
    assert!(stderr.contains(worker.binary), "{stderr}");
    assert!(!stderr.contains("symcockpit"), "{stderr}");
    let names = tool_names(&output);
    assert!(names.contains(&"bootstrap".to_string()), "{names:?}");
    assert!(!names.contains(&worker.tool.to_string()));
}

#[test]
fn enabled_operate_starts_symoperate_serve() {
    enabled_alias_starts_the_worker_and_exposes_its_allowlisted_tools(&OPERATE);
}

#[test]
fn enabled_scope_starts_symscope_serve() {
    enabled_alias_starts_the_worker_and_exposes_its_allowlisted_tools(&SCOPE);
}

#[test]
fn both_workers_run_side_by_side() {
    let setup = Setup::new();
    setup.install(&OPERATE);
    setup.install(&SCOPE);
    let output = setup.run(&setup.profile(&[&OPERATE, &SCOPE]));
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let names = tool_names(&output);
    assert!(names.contains(&"version".to_string()), "{names:?}");
    assert!(names.contains(&"ports_list".to_string()), "{names:?}");
    assert_eq!(setup.invocations(&OPERATE), [["serve"]]);
    assert_eq!(setup.invocations(&SCOPE), [["serve"]]);
}

#[test]
fn operate_not_enabled_starts_nothing() {
    alias_not_enabled_never_starts_the_worker(&OPERATE);
}

#[test]
fn scope_not_enabled_starts_nothing() {
    alias_not_enabled_never_starts_the_worker(&SCOPE);
}

#[test]
fn missing_symoperate_degrades_without_failing_the_gateway() {
    missing_worker_degrades_with_one_diagnostic(&OPERATE);
}

#[test]
fn missing_symscope_degrades_without_failing_the_gateway() {
    missing_worker_degrades_with_one_diagnostic(&SCOPE);
}
