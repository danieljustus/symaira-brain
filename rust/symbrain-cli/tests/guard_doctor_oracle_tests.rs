//! Integration test: drives every one of the frozen `guard doctor` oracle's
//! seven scenarios through the **native** `symbrain` binary.
//!
//! The setup reproduces `guard/scripts/guard-doctor-oracle/main.go`'s
//! `runCase` exactly — same per-case root layout (`<root>/<id>/home/...`,
//! `<root>/<id>/data/symguard`), same injected `HOME`, `XDG_CONFIG_HOME`,
//! `XDG_DATA_HOME` and `SYMGUARD_CONFIG` — and the same normalization
//! (`<root>` for the temp root, plus the shared header normalization for the
//! two rows no fixture can pin).
//!
//! Nativeness is proved structurally: `SYMBRAIN_GO_BINARY` is never set and
//! `PATH` is an empty directory, so no Go fallback exists. A case that reached
//! the fallback would print the "not ported yet and no Go fallback was found"
//! message instead of a report, which is exactly what the gated cases are
//! asserted to do.

#[path = "common/doctor_header.rs"]
mod doctor_header;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

const FIXTURE_PATH: &str = "../symbrain-guard-core/tests/fixtures/doctor_oracle.json";

/// Cases whose bytes this port deliberately declines to produce, with the
/// reason each one is unreproducible. Every one of them must fall back to Go
/// *before* writing any output — see `guard_doctor.rs`'s module docs.
const GATED_CASES: &[(&str, &str)] = &[
    (
        "config_error",
        "Go prints BurntSushi's own parser text (`toml: line 1: expected '.' or '=' …`)",
    ),
    (
        "audit_log_corrupt_anchor",
        "Go prints encoding/json's own error text (`invalid character 'o' in literal null …`)",
    ),
];

fn fixture() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_PATH);
    let data = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&data).expect("fixture is JSON")
}

/// Mirrors the oracle's own per-case scaffolding.
fn setup(id: &str, case_root: &Path) {
    fs::create_dir_all(case_root.join("home/.config/symguard")).unwrap();
    fs::create_dir_all(case_root.join("data/symguard")).unwrap();
    let config = case_root.join("home/.config/symguard/config.toml");
    let log = case_root.join("data/symguard/audit.log");
    match id {
        "empty_machine" => {}
        "healthy_config" => fs::write(
            &config,
            "[defaults]\nshell = \"allow\"\nread_secret = \"deny\"\n\n[[rules]]\nmatch.server = \"symmemory\"\nmatch.tool = \"memory_search\"\ndecision = \"allow\"\n\n[spawn]\n[[spawn.allowlist]]\npath = \"/usr/bin/true\"\n",
        )
        .unwrap(),
        "config_error" => fs::write(&config, "not [valid = toml").unwrap(),
        "audit_log_without_anchor" => fs::write(&log, "{\"entry_id\":\"1\"}\n").unwrap(),
        "audit_log_corrupt_anchor" => {
            fs::write(&log, "{\"entry_id\":\"1\"}\n").unwrap();
            fs::write(case_root.join("data/symguard/audit.log.anchor"), "not json").unwrap();
        }
        "discovered_server_denied" => {
            let cursor = case_root.join("home/.cursor");
            fs::create_dir_all(&cursor).unwrap();
            fs::write(
                cursor.join("mcp.json"),
                r#"{"mcpServers":{"demo":{"command":"/usr/bin/true","args":["--once"]}}}"#,
            )
            .unwrap();
        }
        "discovered_server_secret_risk" => {
            let cursor = case_root.join("home/.cursor");
            fs::create_dir_all(&cursor).unwrap();
            fs::write(
                cursor.join("mcp.json"),
                r#"{"mcpServers":{"server-with-secret":{"command":"/usr/bin/env","env":{"SECRET_KEY":"literal"}}}}"#,
            )
            .unwrap();
        }
        other => panic!("unknown oracle case {other:?} — the fixture grew a case this test does not set up"),
    }
}

fn run(case_root: &Path) -> Output {
    let home = case_root.join("home");
    let config_home = home.join(".config");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    cmd.env_clear();
    #[cfg(windows)]
    for key in ["SystemRoot", "windir", "PATHEXT", "ComSpec", "SystemDrive"] {
        if let Some(val) = std::env::var_os(key) {
            cmd.env(key, val);
        }
    }
    // Deliberately no SYMBRAIN_GO_BINARY, and an empty PATH: there is no Go
    // binary this run could possibly have used.
    cmd.current_dir(case_root)
        .args(["guard", "doctor"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_DATA_HOME", case_root.join("data"))
        .env(
            "SYMGUARD_CONFIG",
            config_home.join("symguard").join("config.toml"),
        )
        .env("PATH", case_root.join("empty-path"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.output().unwrap()
}

#[test]
fn every_oracle_case_is_native_or_explicitly_gated() {
    let suite = fixture();
    let cases = suite["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), 7, "the frozen oracle has seven scenarios");
    let temp = TempDir::new().unwrap();
    // The oracle rewrites its own mktemp root to <root>; macOS hands out
    // /var/... symlinks to /private/var/..., so canonicalize like it does.
    let root = fs::canonicalize(temp.path()).unwrap();

    let mut native = Vec::new();
    let mut gated = Vec::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let case_root = root.join(id);
        setup(id, &case_root);
        let output = run(&case_root);

        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
        let normalized =
            doctor_header::normalize(&stdout.replace(root.to_str().expect("utf8 root"), "<root>"));

        if let Some((_, reason)) = GATED_CASES.iter().find(|(name, _)| *name == id) {
            assert!(
                stdout.is_empty(),
                "{id}: gated cases must fall back before writing any byte ({reason}), got:\n{stdout}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("no Go fallback was found"),
                "{id}: expected the Go fallback to be attempted, stderr was:\n{stderr}"
            );
            gated.push(id);
            continue;
        }

        let expected: String =
            serde_json::from_str(case["output_json"].as_str().expect("output_json"))
                .expect("output_json holds a JSON string");
        let expected = doctor_header::normalize(&expected);
        assert_eq!(
            normalized, expected,
            "{id}: native bytes differ from the frozen Go oracle"
        );

        let expected_code = case["exit_code"].as_i64().expect("exit_code");
        assert_eq!(
            i64::from(output.status.code().expect("exit code")),
            expected_code,
            "{id}: exit code differs from the frozen Go oracle"
        );
        native.push(id);
    }

    assert_eq!(
        native,
        vec![
            "empty_machine",
            "healthy_config",
            "audit_log_without_anchor",
            "discovered_server_denied",
            "discovered_server_secret_risk",
        ],
        "the set of natively handled cases changed"
    );
    assert_eq!(gated, vec!["config_error", "audit_log_corrupt_anchor"]);
}
