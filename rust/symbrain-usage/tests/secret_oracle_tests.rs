//! Consumes `fixtures/secret_oracle.json` — the Go oracle's frozen bytes for
//! `internal/memory/secrets` — and re-runs every case through the native
//! resolution path against an equivalent fake `symvault` on PATH. Never the
//! real binary, keychain, or network.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Deserialize;
use symbrain_usage::{
    is_secret_reference, is_vault_uri, resolve_reference, resolve_reference_or_env,
    secretref_timeout, set_secretref_timeout,
};

const ORACLE: &str = include_str!("fixtures/secret_oracle.json");
const ORACLE_ENV: &[&str] = &[
    "SECRET_ORACLE_FALLBACK",
    "SECRET_ORACLE_PRESENT",
    "SECRET_ORACLE_ABSENT",
    "SECRET_ORACLE_OR_ENV",
];
const SHIPPED_TIMEOUT: Duration = Duration::from_secs(5);

static CASE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
struct Fixture {
    schema_version: u32,
    default_timeout: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    input: Input,
    #[serde(default)]
    timeout: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    value: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    argv: Option<Vec<String>>,
    #[serde(default)]
    is_vault_uri: Option<bool>,
    #[serde(default)]
    is_secret_reference: Option<bool>,
}

#[derive(Deserialize)]
struct Input {
    value: String,
    #[serde(default)]
    env_fallback: String,
    #[serde(default)]
    env_name: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    symvault: Option<SymvaultSpec>,
}

#[derive(Deserialize)]
struct SymvaultSpec {
    mode: String,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    exit: i32,
    #[serde(default)]
    sleep_ms: u64,
}

fn fixture() -> Fixture {
    serde_json::from_str(ORACLE).expect("secret oracle fixture parses")
}

fn cases(kind: &str) -> Vec<Case> {
    fixture()
        .cases
        .into_iter()
        .filter(|case| case.kind == kind)
        .collect()
}

fn lock() -> MutexGuard<'static, ()> {
    CASE_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Mutating the process environment is `unsafe` in edition 2024. Every caller
/// in this binary holds `CASE_LOCK`, so no other thread observes the swap.
#[allow(unsafe_code)]
fn set_env(name: &str, value: &str) {
    unsafe { std::env::set_var(name, value) };
}

#[allow(unsafe_code)]
fn remove_env(name: &str) {
    unsafe { std::env::remove_var(name) };
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn write_fake_symvault(bin_dir: &Path, args_path: &Path, spec: &SymvaultSpec) {
    use std::fmt::Write;
    let mut script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\n",
        shell_quote(&args_path.to_string_lossy())
    );
    if spec.sleep_ms > 0 {
        // Whole-second sleeps render bare; sub-second values keep three
        // digits so /bin/sleep gets a stable decimal on every Unix.
        let seconds = spec.sleep_ms / 1000;
        let millis = spec.sleep_ms % 1000;
        if millis == 0 {
            let _ = writeln!(script, "exec /bin/sleep {seconds}");
        } else {
            let _ = writeln!(script, "exec /bin/sleep {seconds}.{millis:03}");
        }
    } else {
        let _ = writeln!(script, "printf '%s' {}", shell_quote(&spec.stdout));
        let _ = writeln!(script, "printf '%s' {} >&2", shell_quote(&spec.stderr));
        let _ = writeln!(script, "exit {}", spec.exit);
    }
    let path = bin_dir.join("symvault");
    fs::write(&path, script).expect("fake symvault script");
    let mut permissions = fs::metadata(&path)
        .expect("fake symvault metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("fake symvault mode");
}

struct CaseSetup {
    _dir: tempfile::TempDir,
    args_path: PathBuf,
    old_path: String,
}

fn setup_case(input: &Input) -> CaseSetup {
    let dir = tempfile::tempdir().expect("case tempdir");
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    let args_path = dir.path().join("argv.txt");
    if let Some(spec) = &input.symvault
        && spec.mode == "fake"
    {
        write_fake_symvault(&bin_dir, &args_path, spec);
    }
    let old_path = std::env::var("PATH").unwrap_or_default();
    set_env("PATH", &bin_dir.to_string_lossy());
    for name in ORACLE_ENV {
        remove_env(name);
    }
    for (name, value) in &input.env {
        set_env(name, value);
    }
    CaseSetup {
        _dir: dir,
        args_path,
        old_path,
    }
}

fn teardown(setup: CaseSetup) {
    set_env("PATH", &setup.old_path);
    for name in ORACLE_ENV {
        remove_env(name);
    }
    set_secretref_timeout(SHIPPED_TIMEOUT);
    drop(setup);
}

fn read_argv(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(&text)
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn parse_go_duration(text: &str) -> Duration {
    if let Some(number) = text.strip_suffix("ms") {
        return Duration::from_millis(number.parse().expect("millisecond duration"));
    }
    if let Some(number) = text.strip_suffix('s') {
        return Duration::from_secs(number.parse().expect("second duration"));
    }
    panic!("unsupported Go duration {text}");
}

fn run_case(case: &Case) {
    let _guard = lock();
    let setup = setup_case(&case.input);
    if let Some(timeout) = &case.timeout {
        set_secretref_timeout(parse_go_duration(timeout));
    }
    let result = match case.kind.as_str() {
        "resolve" | "timeout" => resolve_reference(&case.input.value, &case.input.env_fallback),
        "resolve_or_env" => resolve_reference_or_env(&case.input.value, &case.input.env_name),
        other => panic!("unexpected case kind {other}"),
    };
    let argv = read_argv(&setup.args_path);
    match (case.success, result) {
        (Some(true), Ok(value)) => assert_eq!(value, case.value, "success value for {}", case.id),
        (Some(false), Err(error)) => assert_eq!(error, case.error, "error bytes for {}", case.id),
        (Some(true), Err(error)) => panic!("{}: expected success, got error {error}", case.id),
        (Some(false), Ok(value)) => panic!("{}: expected error, got value {value}", case.id),
        (None, _) => panic!("{}: fixture case lacks success flag", case.id),
    }
    assert_eq!(
        argv,
        case.argv.clone().unwrap_or_default(),
        "argv for {}",
        case.id
    );
    teardown(setup);
}

#[test]
fn scheme_acceptance_matches_go_classification() {
    let all = cases("scheme");
    assert_eq!(all.len(), 9, "scheme cases");
    for case in &all {
        assert_eq!(
            is_vault_uri(&case.input.value),
            case.is_vault_uri.unwrap_or(false),
            "is_vault_uri for {}",
            case.id
        );
        assert_eq!(
            is_secret_reference(&case.input.value),
            case.is_secret_reference.unwrap_or(false),
            "is_secret_reference for {}",
            case.id
        );
    }
}

#[test]
fn resolve_value_and_error_bytes_match_go() {
    let all = cases("resolve");
    assert_eq!(all.len(), 13, "resolve cases");
    for case in &all {
        run_case(case);
    }
}

#[test]
fn resolve_or_env_precedence_matches_go() {
    let all = cases("resolve_or_env");
    assert_eq!(all.len(), 4, "resolve_or_env cases");
    for case in &all {
        run_case(case);
    }
}

#[test]
fn shrunk_timeout_error_bytes_match_go() {
    let all = cases("timeout");
    assert_eq!(all.len(), 1, "timeout cases");
    for case in &all {
        run_case(case);
    }
}

#[test]
fn alias_and_canonical_resolve_identically() {
    let parsed = fixture();
    let canonical = parsed
        .cases
        .iter()
        .find(|case| case.id == "canonical_ok")
        .expect("canonical case");
    let alias = parsed
        .cases
        .iter()
        .find(|case| case.id == "alias_ok_equivalent")
        .expect("alias case");
    assert_eq!(
        canonical.value, alias.value,
        "deprecated alias must yield the canonical value"
    );
    let argv = canonical.argv.as_ref().expect("canonical argv");
    assert_eq!(
        alias.argv.as_deref(),
        Some(argv.as_slice()),
        "deprecated alias must issue identical argv"
    );
    assert_eq!(argv, &["get", "--", "symaira/memory/jwt", "--print"]);
}

#[test]
fn fixture_pins_timeout_surface_count_and_ids() {
    let _guard = lock();
    let parsed = fixture();
    assert_eq!(parsed.schema_version, 1, "fixture schema");
    assert_eq!(parsed.default_timeout, "5s", "Go default timeout surface");
    assert_eq!(
        secretref_timeout(),
        SHIPPED_TIMEOUT,
        "native default timeout"
    );
    assert_eq!(parsed.cases.len(), 27, "fixture case count");
    let mut ids: Vec<&str> = parsed.cases.iter().map(|case| case.id.as_str()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "case ids must be unique");
    set_secretref_timeout(SHIPPED_TIMEOUT);
}
