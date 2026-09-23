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
    value_bytes: Vec<u8>,
    #[serde(default)]
    error: String,
    #[serde(default)]
    argv: Option<Vec<String>>,
    #[serde(default)]
    is_vault_uri: Option<bool>,
    #[serde(default)]
    is_secret_reference: Option<bool>,
    #[serde(default)]
    value_leaks: Option<ValueLeaks>,
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
    stdout_bytes: Vec<u8>,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    exit: i32,
    #[serde(default)]
    sleep_ms: u64,
}

/// Go's `valueLeakRecord`: which recorded channels of a failed case contain
/// a setup plaintext. Mirrors scripts/secret-oracle/main.go field-for-field;
/// every shipped failure case records all-false (resolve.go:44: the secret
/// value is never included in error messages).
// The four channel verdicts are a frozen cross-language JSON contract
// (Go's `valueLeakRecord`); enums would break the fixture schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Deserialize, Debug, PartialEq)]
struct ValueLeaks {
    plaintexts: Vec<String>,
    error: bool,
    stdout: bool,
    stderr: bool,
    argv: bool,
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
    // Canned output is written before any sleep so a timeout probe
    // exercises "value obtained, then deadline" like the Go helper.
    let _ = writeln!(script, "printf '%s' {}", shell_quote(&spec.stdout));
    if !spec.stdout_bytes.is_empty() {
        let mut octal = String::new();
        for byte in &spec.stdout_bytes {
            let _ = write!(octal, "\\{byte:03o}");
        }
        let _ = writeln!(script, "printf '%b' {}", shell_quote(&octal));
    }
    let _ = writeln!(script, "printf '%s' {} >&2", shell_quote(&spec.stderr));
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
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    dir: tempfile::TempDir,
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
        dir,
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

/// Go's `setupPlaintexts`: injected env values (`BTreeMap` iteration is
/// already key-sorted, matching Go's explicit sort) plus the trimmed secret
/// the fake `symvault` prints on stdout. Fake stderr is the diagnostic
/// channel corekit folds into errors by design — never a plaintext source.
fn setup_plaintexts(input: &Input) -> Vec<String> {
    let mut plaintexts: Vec<String> = input
        .env
        .values()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    if let Some(spec) = &input.symvault
        && spec.mode == "fake"
    {
        let printed = spec.stdout.trim();
        if !printed.is_empty() {
            plaintexts.push(printed.to_owned());
        }
    }
    plaintexts
}

/// Go's `computeValueLeaks`: scan a case's recorded channels for its setup
/// plaintexts. `value` is the stdout-derived result (always empty on
/// failure); `stderr` bytes are the recorded subprocess stderr.
fn compute_value_leaks(case: &Case, error: &str, value: &str, argv: &[String]) -> ValueLeaks {
    let plaintexts = setup_plaintexts(&case.input);
    let stderr = case
        .input
        .symvault
        .as_ref()
        .map_or("", |spec| spec.stderr.as_str());
    let (mut error_leak, mut stdout_leak, mut stderr_leak, mut argv_leak) =
        (false, false, false, false);
    for plaintext in &plaintexts {
        error_leak |= error.contains(plaintext.as_str());
        stdout_leak |= value.contains(plaintext.as_str());
        stderr_leak |= stderr.contains(plaintext.as_str());
        argv_leak |= argv.iter().any(|arg| arg.contains(plaintext.as_str()));
    }
    ValueLeaks {
        plaintexts,
        error: error_leak,
        stdout: stdout_leak,
        stderr: stderr_leak,
        argv: argv_leak,
    }
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
        (Some(false), Err(error)) => {
            assert_eq!(error, case.error, "error bytes for {}", case.id);
            // The invariant, checked against the native run's own bytes:
            // no setup plaintext may surface in error/value/argv.
            let leaks = compute_value_leaks(case, &error, "", &argv);
            assert!(
                !leaks.error && !leaks.stdout && !leaks.stderr && !leaks.argv,
                "setup plaintext leaked into {}'s native failure record: {leaks:?}",
                case.id
            );
        }
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
    assert_eq!(all.len(), 15, "resolve cases");
    for case in all.iter().filter(|case| case.value_bytes.is_empty()) {
        run_case(case);
    }
}

#[test]
fn non_utf8_secret_fails_closed_instead_of_silently_changing_bytes() {
    let parsed = fixture();
    let case = parsed
        .cases
        .iter()
        .find(|case| case.id == "non_utf8_symvault_stdout")
        .expect("Go-generated raw-byte case");
    assert_eq!(case.success, Some(true), "Go returned the raw secret");
    let spec = case.input.symvault.as_ref().expect("fake subprocess");
    assert_eq!(spec.stdout_bytes.last(), Some(&b'\n'));
    assert_eq!(
        case.value_bytes,
        spec.stdout_bytes[..spec.stdout_bytes.len() - 1]
    );
    assert!(std::str::from_utf8(&case.value_bytes).is_err());

    let _guard = lock();
    let setup = setup_case(&case.input);
    let error = resolve_reference(&case.input.value, "").expect_err("must reject invalid UTF-8");
    assert_eq!(
        error,
        "secret resolution failed for symvault://raw/bytes: resolve symvault://raw/bytes: non-UTF-8 secret output; set env var  as fallback or install symvault"
    );
    assert!(!error.contains(char::REPLACEMENT_CHARACTER));
    assert_eq!(
        read_argv(&setup.args_path),
        case.argv.clone().unwrap_or_default()
    );
    teardown(setup);
}

#[cfg(target_os = "macos")]
#[test]
fn non_utf8_keychain_stdout_uses_the_same_fail_closed_decoder() {
    let parsed = fixture();
    let case = parsed
        .cases
        .iter()
        .find(|case| case.id == "non_utf8_symvault_stdout")
        .expect("Go-generated raw-byte input");
    let _guard = lock();
    let setup = setup_case(&case.input);
    // This PATH-local stand-in cannot access the operator's login keychain.
    fs::copy(
        setup.dir.path().join("bin/symvault"),
        setup.dir.path().join("bin/security"),
    )
    .expect("fake security executable");
    let error = resolve_reference("keychain://fake/account", "")
        .expect_err("keychain must reject invalid UTF-8 too");
    assert!(error.contains("resolve keychain://fake/account: non-UTF-8 secret output"));
    assert!(!error.contains(char::REPLACEMENT_CHARACTER));
    assert_eq!(
        read_argv(&setup.args_path),
        ["find-generic-password", "-w", "-s", "fake", "-a", "account"]
    );
    teardown(setup);
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
    assert_eq!(all.len(), 2, "timeout cases");
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
    assert_eq!(parsed.cases.len(), 30, "fixture case count");
    let mut ids: Vec<&str> = parsed.cases.iter().map(|case| case.id.as_str()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "case ids must be unique");
    set_secretref_timeout(SHIPPED_TIMEOUT);
}

#[test]
fn failed_cases_pin_the_no_plaintext_leak_invariant() {
    let parsed = fixture();
    let failures: Vec<&Case> = parsed
        .cases
        .iter()
        .filter(|case| case.success == Some(false))
        .collect();
    assert_eq!(failures.len(), 12, "failed cases");
    let mut carrying = 0;
    for case in failures {
        let recorded = case
            .value_leaks
            .as_ref()
            .unwrap_or_else(|| panic!("failure case {} must record value_leaks", case.id));
        let argv = case.argv.as_deref().unwrap_or(&[]);
        let recomputed = compute_value_leaks(case, &case.error, &case.value, argv);
        assert_eq!(&recomputed, recorded, "value_leaks for {}", case.id);
        assert!(!recorded.error, "error channel leaks for {}", case.id);
        assert!(!recorded.stdout, "stdout channel leaks for {}", case.id);
        assert!(!recorded.stderr, "stderr channel leaks for {}", case.id);
        assert!(!recorded.argv, "argv channel leaks for {}", case.id);
        if !recorded.plaintexts.is_empty() {
            carrying += 1;
        }
    }
    // The env-fallback failure plus both echo probes must carry plaintexts,
    // so the invariant is exercised rather than vacuous.
    assert!(
        carrying >= 3,
        "expected at least 3 failure cases carrying plaintexts, got {carrying}"
    );
}
