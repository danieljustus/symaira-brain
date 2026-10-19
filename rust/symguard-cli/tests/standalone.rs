//! Exercise the independently shipped process with disposable Guard state.
use std::fs;
use std::io::{Read, Seek, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn run(root: &TempDir, args: &[&str], input: &[u8]) -> Output {
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let stdout = tempfile::tempfile_in(root.path()).unwrap();
    let stderr = tempfile::tempfile_in(root.path()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_symguard"));
    cmd.env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("SYMBRAIN_GO_BINARY", root.path().join("absent-go"))
        .env("PATH", "")
        .current_dir(root.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()));
    for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("standalone Guard exceeded process deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut out = stdout;
    let mut err = stderr;
    out.rewind().unwrap();
    err.rewind().unwrap();
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    out.read_to_end(&mut output.stdout).unwrap();
    err.read_to_end(&mut output.stderr).unwrap();
    output
}

#[test]
fn standalone_routes_and_handshake_do_not_require_brain_or_go() {
    let root = TempDir::new().unwrap();
    let help = run(&root, &["help"], b"");
    assert!(help.status.success() && help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("symguard <command>")
    );
    let scan = run(&root, &["scan", "--format=json"], b"");
    assert!(scan.status.success());
    let inventory: serde_json::Value = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(inventory["servers"], serde_json::json!([]));
    let version = run(&root, &["version", "--json"], b"");
    assert!(version.status.success() && version.stderr.is_empty());
    let version: serde_json::Value = serde_json::from_slice(&version.stdout).unwrap();
    assert_eq!(version["tool"], "symguard");
    assert_eq!(version["schema_version"], 1);
    let unknown = run(&root, &["proxy"], b"");
    assert_eq!(unknown.status.code(), Some(2));
    assert!(unknown.stdout.is_empty());
}

#[test]
fn standalone_decide_keeps_private_guard_audit_state() {
    let root = TempDir::new().unwrap();
    let output = run(
        &root,
        &["decide"],
        br#"{"command":"fixture","risk_class":"low"}"#,
    );
    assert!(output.status.success() && output.stderr.is_empty());
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["decision"], "allow");
    let audit = root.path().join("data/symguard/audit.log");
    let records = fs::read_to_string(&audit).unwrap();
    assert_eq!(records.lines().count(), 1);
    let record: serde_json::Value = serde_json::from_str(records.trim()).unwrap();
    assert_eq!(record["command"], "fixture");
    assert_eq!(record["decision"], "allow");
    assert!(!root.path().join("data/symbrain").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&audit).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(audit.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn wrongly_typed_doctor_config_is_a_native_parse_error() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config/symguard");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        "[sequence]\nenabled=true\nthreshold=\"wrong\"\n",
    )
    .unwrap();
    let result = run(&root, &["doctor"], b"");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(
        stdout.lines().any(
            |line| line.starts_with("  config           error: config: parse ")
                && line.ends_with(
                    ": toml: a known field has an unsupported type or a case-insensitive alias"
                )
        ),
        "{stdout}"
    );
    assert!(stdout.contains("  policy           not loaded (config error)\n"));
    assert!(result.stderr.is_empty());
    assert!(!root.path().join("data").exists());
}

fn doctor_with_config(text: &str) -> (Option<i32>, String) {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config/symguard");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), text).unwrap();
    let result = run(&root, &["doctor"], b"");
    assert!(result.stderr.is_empty(), "{:?}", result.stderr);
    (
        result.status.code(),
        String::from_utf8(result.stdout).unwrap(),
    )
}

fn config_row(stdout: &str) -> &str {
    stdout
        .lines()
        .find(|line| line.starts_with("  config "))
        .expect("config row")
}

#[test]
fn unported_toml_syntax_errors_are_native_parse_errors() {
    let (code, stdout) = doctor_with_config("not [valid = toml\n");
    assert_eq!(code, Some(1));
    let row = config_row(&stdout);
    assert!(
        row.contains("error: config: parse ") && row.contains(": toml: line 1: "),
        "{row}"
    );
}

#[test]
fn several_invalid_defaults_report_the_first_key_deterministically() {
    let text = "[defaults]\nzeta = \"bad\"\nalpha = \"worse\"\nmid = \"also\"\n";
    let (code, first) = doctor_with_config(text);
    assert_eq!(code, Some(1));
    assert!(
        config_row(&first).ends_with(": defaults.\"alpha\": invalid decision \"worse\" (allowed: allow, ask, deny, redact, readonly, sandbox)"),
        "{first}"
    );
    // Identical apart from the per-run temporary config path.
    let detail = |stdout: &str| {
        config_row(stdout)
            .rsplit_once("config.toml")
            .unwrap()
            .1
            .to_owned()
    };
    assert_eq!(detail(&doctor_with_config(text).1), detail(&first));
}

#[cfg(unix)]
#[test]
fn unreadable_audit_log_is_a_native_stat_error() {
    let root = TempDir::new().unwrap();
    // The audit log's parent is a regular file, so stat fails with ENOTDIR.
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/symguard"), b"").unwrap();
    let result = run(&root, &["doctor"], b"");
    let stdout = String::from_utf8(result.stdout).unwrap();
    let row = stdout
        .lines()
        .find(|line| line.starts_with("  audit log "))
        .unwrap();
    assert!(
        row.contains("error: stat ") && row.ends_with("/symguard/audit.log: Not a directory"),
        "{row}"
    );
    assert_eq!(result.status.code(), Some(1));
}

// Windows reports missing parent directories of other sources first.
#[cfg(unix)]
#[test]
fn server_without_command_or_url_is_a_native_discovery_error() {
    let root = TempDir::new().unwrap();
    let cursor = root.path().join("home/.cursor");
    fs::create_dir_all(&cursor).unwrap();
    fs::write(cursor.join("mcp.json"), br#"{"mcpServers":{"empty":{}}}"#).unwrap();
    let result = run(&root, &["doctor"], b"");
    let stdout = String::from_utf8(result.stdout).unwrap();
    let row = stdout
        .lines()
        .find(|line| line.starts_with("  mcp servers "))
        .unwrap();
    assert!(
        row.contains("error: discovery: cursor (")
            && row.ends_with("): [unsupported] server \"empty\" is missing both command and url"),
        "{row}"
    );
    assert_eq!(result.status.code(), Some(1));
}
