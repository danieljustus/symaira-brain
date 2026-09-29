//! Provider credential state machine and per-source auth texts.
//!
//! Mirrors `internal/usage/{claude,codex,copilot,cursor,kimi,moonshot,nous,
//! opencode,openrouter,antigravity}.go`, including the exact missing/expired/
//! available texts and the source tag each state reports. Resolution order per
//! provider is: environment variable (symvault/keychain capable), then the
//! provider's own credential file, then — Claude on macOS only — the login
//! keychain. Claude, Codex, Copilot, Kimi CLI, and Hermes files use native
//! reporting only for proven deterministic shapes at their default or
//! supported home paths. Supported providers may be combined; any unproven
//! source keeps the report on Go.

use super::provider_requests::{trusted_https_url, validated_base};
use super::{AuthStatus, MAX_CREDENTIAL_FILE_BYTES, Provider, Value};
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::env;
use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// The service Claude Code stores its OAuth credentials under in the macOS
/// login keychain. Current versions append a per-installation hex suffix that
/// is not derivable from disk, so the bare name is tried first and every
/// suffixed variant found afterwards.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
/// Reading an item this binary is not on the ACL of raises an approval panel
/// that blocks the subprocess until answered; a usage report must not hang on
/// an unattended machine.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CLAUDE_KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// Bound for the prompt-free macOS Keychain listing.
#[cfg(target_os = "macos")]
const MAX_PROBE_OUTPUT_BYTES: u64 = 64 * 1024;

// ---------------------------------------------------------------------------
// Credential resolution (mirrors internal/usage/creds.go)
// ---------------------------------------------------------------------------

fn env_raw(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn secret_reference_source(reference: &str) -> &'static str {
    if reference.starts_with("env://") {
        "env"
    } else if reference.starts_with("keychain://") {
        "keychain"
    } else {
        "vault"
    }
}

/// Returns true for any Brain secret-reference scheme: `symvault://`, the
/// deprecated `vault://` alias, `env://`, or `keychain://` — Go's
/// `secrets.IsSecretReference` contract.
#[must_use]
pub fn is_secret_reference(value: &str) -> bool {
    ["symvault://", "vault://", "env://", "keychain://"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

/// Returns true for the canonical `symvault://` scheme or the deprecated
/// `vault://` alias — Go's `secrets.IsVaultURI` contract, which gates the
/// environment fallback in [`resolve_reference`].
#[must_use]
pub fn is_vault_uri(value: &str) -> bool {
    value.starts_with("symvault://") || value.starts_with("vault://")
}

/// Resolves one environment-sourced credential.
///
/// `Ok(None)` means unset (the caller falls back to its file source); `Ok` with
/// a plain value reports source `env`; a secret reference is resolved by the
/// shared resolver and reports the reference's own source. `Err` carries the
/// text the shipped implementation reports as `credential resolution failed`.
fn resolve_env(name: &str) -> Result<Option<(String, String)>, String> {
    let Some(raw) = env_raw(name) else {
        return Ok(None);
    };
    if !is_secret_reference(&raw) {
        return Ok(Some(("env".into(), raw)));
    }
    let source = secret_reference_source(&raw);
    let resolved = if let Some(reference) = raw.strip_prefix("env://") {
        env_raw(reference)
    } else if let Some(reference) = raw.strip_prefix("keychain://") {
        reference
            .split_once('/')
            .filter(|(service, account)| !service.is_empty() && !account.is_empty())
            .and_then(|(service, account)| {
                resolve_secret_command(
                    "security",
                    &["find-generic-password", "-w", "-s", service, "-a", account],
                )
            })
    } else {
        raw.strip_prefix("symvault://")
            .or_else(|| raw.strip_prefix("vault://"))
            .filter(|path| validate_vault_path(path).is_ok())
            .and_then(|path| resolve_secret_command("symvault", &["get", "--", path, "--print"]))
    };
    match resolved {
        Some(value) => Ok(Some((source.into(), value))),
        None => Err(format!(
            "resolve {name}: secret resolution failed for {raw}"
        )),
    }
}

/// Resolves a credential with a file-based fallback: the environment variable
/// (symvault-capable) wins, otherwise `read_file` is tried.
fn resolve_env_or_file(
    name: &str,
    read_file: impl Fn() -> Option<String>,
) -> Result<Option<(String, String)>, String> {
    if let Some((source, value)) = resolve_env(name)? {
        return Ok(Some((source, value)));
    }
    Ok(read_file().map(|value| ("file".into(), value)))
}

fn resolve_secret_command(command: &str, args: &[&str]) -> Option<String> {
    let output = run_command_capture(
        command,
        args,
        secretref_timeout(),
        MAX_CREDENTIAL_FILE_BYTES,
    )
    .ok()?;
    String::from_utf8(output)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Why a bounded child command did not yield usable stdout.
enum CommandFailure {
    NotFound,
    TimedOut,
    ExitFailed { code: Option<i32>, stderr: Vec<u8> },
    Other(String),
}

/// Runs one command with a hard deadline and bounded stdout/stderr capture,
/// never through a shell. Each stream goes to a temporary file so a child
/// holding a pipe open cannot outlive cancellation. This is the single
/// subprocess runner behind both the legacy `Option` API and the
/// Go-parity secret-reference path.
fn run_command_capture(
    command: &str,
    args: &[&str],
    timeout: Duration,
    cap: u64,
) -> Result<Vec<u8>, CommandFailure> {
    let file =
        tempfile::NamedTempFile::new().map_err(|error| CommandFailure::Other(error.to_string()))?;
    let stderr_file =
        tempfile::NamedTempFile::new().map_err(|error| CommandFailure::Other(error.to_string()))?;
    let handle = file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let stderr_handle = stderr_file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let mut child_command = Command::new(command);
    child_command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::from(stderr_handle))
        .stdout(Stdio::from(handle));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut child_command, 0);
    let mut child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CommandFailure::NotFound);
        }
        Err(error) => return Err(CommandFailure::Other(error.to_string())),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = read_capture(stderr_file.as_file(), cap);
                    return Err(CommandFailure::ExitFailed {
                        code: status.code(),
                        stderr,
                    });
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                terminate_child(&mut child);
                return Err(CommandFailure::TimedOut);
            }
            Err(error) => {
                terminate_child(&mut child);
                return Err(CommandFailure::Other(error.to_string()));
            }
        }
    }
    terminate_child(&mut child);
    let mut file = file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let mut output = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut output)
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    if output.len() as u64 > cap {
        return Err(CommandFailure::Other(
            "command output exceeds the bounded read limit".into(),
        ));
    }
    Ok(output)
}

fn read_capture(file: &std::fs::File, cap: u64) -> Vec<u8> {
    let Ok(mut file) = file.try_clone() else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(0)).is_err() {
        return Vec::new();
    }
    let mut buffer = Vec::new();
    if file.take(cap + 1).read_to_end(&mut buffer).is_err() {
        return Vec::new();
    }
    if buffer.len() as u64 > cap {
        buffer.truncate(usize::try_from(cap).unwrap_or(usize::MAX));
    }
    buffer
}

/// Legacy surface: probes and the usage credential path keep their
/// `Option<Vec<u8>>` contract over the shared runner.
#[cfg(target_os = "macos")]
fn bounded_command_stdout(
    command: &str,
    args: &[&str],
    timeout: Duration,
    cap: u64,
) -> Option<Vec<u8>> {
    run_command_capture(command, args, timeout, cap).ok()
}

fn terminate_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(child),
            rustix::process::Signal::KILL,
        );
    }
    let _ = child.kill();
    let _ = child.wait();
}

// ---------------------------------------------------------------------------
// Secret-reference resolution: freezes internal/memory/secrets + corekit's
// secretref byte contract, verified by tests/secret_oracle_tests.rs against
// the Go-generated fixtures/secret_oracle.json.
// ---------------------------------------------------------------------------

/// Mirrors corekit's exported `secretref.DefaultTimeout` variable: the bound
/// every `symvault`/`security` subprocess runs under. Shrinkable so tests can
/// exercise deadline bytes without sleeping the shipped 5s.
static SECRETREF_TIMEOUT_MS: AtomicU64 = AtomicU64::new(5_000);

/// The current secret subprocess timeout (shipped default: 5 seconds).
#[must_use]
pub fn secretref_timeout() -> Duration {
    Duration::from_millis(SECRETREF_TIMEOUT_MS.load(Ordering::Relaxed))
}

/// Replaces the secret subprocess timeout, mirroring Go's assignment to
/// `secretref.DefaultTimeout`. Tests restore it with `Duration::from_secs(5)`.
pub fn set_secretref_timeout(timeout: Duration) {
    let milliseconds = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    SECRETREF_TIMEOUT_MS.store(milliseconds, Ordering::Relaxed);
}

/// Renders a duration the way Go's `time.Duration.String` renders the whole
/// second / whole millisecond values this surface exposes: "5s", "150ms".
fn go_duration(timeout: Duration) -> String {
    let milliseconds = timeout.as_millis();
    if milliseconds.is_multiple_of(1_000) {
        format!("{}s", milliseconds / 1_000)
    } else {
        format!("{milliseconds}ms")
    }
}

/// corekit's `validateVaultPath`, checked before the binary is looked up.
fn validate_vault_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("invalid symvault credential path: empty".to_owned());
    }
    if path.starts_with('-') {
        return Err("invalid symvault credential path: must not start with '-'".to_owned());
    }
    if path.contains('\0') {
        return Err("invalid symvault credential path: contains a null byte".to_owned());
    }
    if path.chars().any(char::is_control) {
        return Err("invalid symvault credential path: contains control characters".to_owned());
    }
    Ok(())
}

/// corekit's `refLabel`: empty references report `<default>`.
fn reference_label(reference: &str) -> &str {
    if reference.is_empty() {
        "<default>"
    } else {
        reference
    }
}

/// corekit's subprocess failure texts: symvault's `LookPath` message, the
/// Go-wrapped deadline message, and `exit status N[: stderr detail]`.
fn command_failure_message(
    failure: CommandFailure,
    action: &str,
    binary: &str,
    timeout: Duration,
) -> String {
    match failure {
        CommandFailure::NotFound if binary == "symvault" => {
            "symvault binary not found on PATH".to_owned()
        }
        CommandFailure::NotFound => format!("exec: {binary:?}: executable file not found in $PATH"),
        CommandFailure::TimedOut => format!(
            "secretref: subprocess timed out: {action} timed out after {}",
            go_duration(timeout)
        ),
        CommandFailure::ExitFailed { code, stderr } => {
            let status = code.map_or_else(
                || "signal: killed".to_owned(),
                |code| format!("exit status {code}"),
            );
            let detail = String::from_utf8_lossy(&stderr).trim().to_owned();
            if detail.is_empty() {
                status
            } else {
                format!("{status}: {detail}")
            }
        }
        CommandFailure::Other(message) => message,
    }
}

/// corekit's `secretref.Resolve` dispatcher. Go always passes an empty
/// default, so the bare-name branch exists only for byte parity.
fn resolve_shared_reference(reference: &str) -> Result<String, String> {
    if reference.is_empty() {
        return Err("no credential reference or default provided".to_owned());
    }
    if let Some(path) = reference.strip_prefix("symvault://") {
        return resolve_symvault_reference(reference, path);
    }
    if let Some(rest) = reference.strip_prefix("keychain://") {
        return match rest.split_once('/') {
            Some((service, account)) if !service.is_empty() && !account.is_empty() => {
                resolve_keychain_reference(reference, service, account)
            }
            _ => Err(format!(
                "resolve {}: invalid keychain reference, expected keychain://service/account",
                reference_label(reference)
            )),
        };
    }
    if let Some(name) = reference.strip_prefix("env://") {
        return match env_raw(name) {
            Some(value) => Ok(value),
            None => Err(format!(
                "environment variable {name} is not set (reference {})",
                reference_label(reference)
            )),
        };
    }
    Err(format!(
        "environment variable {reference} is not set (reference {})",
        reference_label(reference)
    ))
}

fn resolve_symvault_reference(reference: &str, path: &str) -> Result<String, String> {
    let label = reference_label(reference);
    validate_vault_path(path).map_err(|error| format!("resolve {label}: {error}"))?;
    let timeout = secretref_timeout();
    let output = run_command_capture(
        "symvault",
        &["get", "--", path, "--print"],
        timeout,
        MAX_CREDENTIAL_FILE_BYTES,
    )
    .map_err(|failure| {
        format!(
            "resolve {label}: {}",
            command_failure_message(failure, "symvault get", "symvault", timeout)
        )
    })?;
    decode_secret_stdout(output, label)
}

fn decode_secret_stdout(output: Vec<u8>, label: &str) -> Result<String, String> {
    // The public API returns String. Reject unrepresentable bytes rather than
    // silently changing an authentication secret with replacement characters.
    String::from_utf8(output)
        .map(|value| value.trim().to_owned())
        .map_err(|_| format!("resolve {label}: non-UTF-8 secret output"))
}

/// Go spawns the bare `security` binary with no pre-lookup, so only macOS
/// resolves `keychain://` references.
#[cfg(target_os = "macos")]
fn resolve_keychain_reference(
    reference: &str,
    service: &str,
    account: &str,
) -> Result<String, String> {
    let label = reference_label(reference);
    let timeout = secretref_timeout();
    let output = run_command_capture(
        "security",
        &["find-generic-password", "-w", "-s", service, "-a", account],
        timeout,
        MAX_CREDENTIAL_FILE_BYTES,
    )
    .map_err(|failure| {
        format!(
            "resolve {label}: {}",
            command_failure_message(failure, "keychain lookup", "security", timeout)
        )
    })?;
    decode_secret_stdout(output, label)
}

#[cfg(not(target_os = "macos"))]
fn resolve_keychain_reference(
    reference: &str,
    _service: &str,
    _account: &str,
) -> Result<String, String> {
    Err(format!(
        "resolve {}: keychain:// references are only resolvable on macOS",
        reference_label(reference)
    ))
}

/// Go's `internal/memory/secrets.Resolve`: plain values stay literal,
/// `vault://` is a deprecated alias for `symvault://`, and vault failures
/// fall back to `env_fallback` when it names a non-empty environment
/// variable. Error bytes match Go exactly and never contain the secret.
///
/// # Errors
/// Returns the Go-identical wrapped resolution error.
pub fn resolve_reference(value: &str, env_fallback: &str) -> Result<String, String> {
    if value.is_empty() || !is_secret_reference(value) {
        return Ok(value.to_owned());
    }
    let reference = match value.strip_prefix("vault://") {
        Some(rest) => format!("symvault://{rest}"),
        None => value.to_owned(),
    };
    let failure = match resolve_shared_reference(&reference) {
        Ok(secret) if !secret.is_empty() => return Ok(secret),
        Ok(_) => "shared resolver returned empty secret".to_owned(),
        Err(message) => message,
    };
    if is_vault_uri(value)
        && !env_fallback.is_empty()
        && let Some(fallback) = env_raw(env_fallback)
    {
        return Ok(fallback);
    }
    Err(format!(
        "secret resolution failed for {reference}: {failure}; set env var {env_fallback} as fallback or install symvault"
    ))
}

/// Go's `internal/memory/secrets.ResolveOrEnv`: a non-empty value resolves
/// through [`resolve_reference`] with `env_name` as its fallback; an empty
/// value reads `env_name` directly, defaulting to an empty string.
///
/// # Errors
/// Propagates [`resolve_reference`]'s Go-identical error bytes.
pub fn resolve_reference_or_env(value: &str, env_name: &str) -> Result<String, String> {
    if !value.is_empty() {
        return resolve_reference(value, env_name);
    }
    Ok(env_raw(env_name).unwrap_or_default())
}

fn read_limited(path: &Path) -> Option<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW);
    let file = options.open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_CREDENTIAL_FILE_BYTES {
        return None;
    }
    let mut contents = Vec::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    (contents.len() <= usize::try_from(MAX_CREDENTIAL_FILE_BYTES).ok()?).then_some(contents)
}

fn json_value(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_limited(path)?).ok()
}

fn json_string(path: &Path, pointers: &[&str]) -> Option<String> {
    json_value(path)?
        .pointer(&format!("/{}", pointers.join("/")))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(Into::into)
}

fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn env_path(name: &str, fallback: PathBuf) -> PathBuf {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map_or(fallback, PathBuf::from)
}

// ---------------------------------------------------------------------------
// Provider credential files
// ---------------------------------------------------------------------------

/// `~/.claude/.credentials.json`, accepting only shapes whose token choice is
/// deterministic and matches Go: the default account, or exactly one other
/// account with a nonempty token. Unknown metadata and other unproven shapes
/// remain on the Go path.
fn claude_file_token() -> Option<String> {
    claude_file_token_in(&home().join(".claude/.credentials.json"))
}

fn claude_file_token_in(path: &Path) -> Option<String> {
    let contents = read_limited(path)?;
    let credentials: ClaudeCredentialFile = serde_json::from_slice(&contents).ok()?;
    let accounts = credentials.oauth_account;
    if let Some(token) = accounts
        .get("default")
        .and_then(|account| account.access_token.as_deref())
        .filter(|token| !token.is_empty())
    {
        return Some(token.to_owned());
    }
    let mut tokens = accounts
        .values()
        .filter_map(|account| account.access_token.as_deref())
        .filter(|token| !token.is_empty());
    let token = tokens.next()?;
    tokens.next().is_none().then(|| token.to_owned())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaudeCredentialFile {
    #[serde(rename = "oauthAccount")]
    oauth_account: std::collections::BTreeMap<String, ClaudeOAuthAccount>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaudeOAuthAccount {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
}

fn codex_home() -> PathBuf {
    env_path("CODEX_HOME", home().join(".codex"))
}

/// `$CODEX_HOME/auth.json`: a top-level `access_token` or the nested
/// `tokens.access_token` the newer CLI writes.
fn codex_file_token(home_dir: &Path) -> Option<String> {
    let root = json_value(&home_dir.join("auth.json"))?;
    root.get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .or_else(|| {
            root.get("tokens")?
                .get("access_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
        })
        .map(Into::into)
}

fn copilot_config_dir() -> PathBuf {
    copilot_config_dir_for(&home())
}

fn copilot_config_dir_for(home: &Path) -> PathBuf {
    home.join(".config/github-copilot")
}

/// Best-effort provider parser for `apps.json` then `hosts.json`; the CLI's
/// native route separately requires a strict Go-equivalent candidate below.
fn copilot_file_token() -> Option<String> {
    copilot_file_token_in(&copilot_config_dir())
}

fn copilot_file_token_in(dir: &Path) -> Option<String> {
    for name in ["apps.json", "hosts.json"] {
        let Some(root) = json_value(&dir.join(name)) else {
            continue;
        };
        for prefix in ["github.com:", ""] {
            let Some(entries) = root.as_object() else {
                break;
            };
            if let Some(token) = entries.iter().find_map(|(key, entry)| {
                (prefix.is_empty() || key.starts_with(prefix))
                    .then(|| {
                        entry
                            .get("oauth_token")
                            .and_then(Value::as_str)
                            .filter(|token| !token.is_empty())
                    })
                    .flatten()
            }) {
                return Some(token.to_owned());
            }
        }
    }
    None
}

/// `Ok(None)` means both files are absent. `Err(())` means an existing source
/// cannot be proven to have the same token selection as Go and must be routed
/// to the Go implementation before constructing a provider.
fn copilot_file_token_candidate_in(dir: &Path) -> Result<Option<String>, ()> {
    let apps_path = dir.join("apps.json");
    if let Some(contents) = read_optional_credential_file(&apps_path)? {
        // Go checks apps.json before hosts.json and returns its first usable
        // token. An empty or unknown apps file can affect whether hosts.json
        // is reached, so leave every such case to Go.
        return parse_single_copilot_token(&contents).map(Some);
    }

    let hosts_path = dir.join("hosts.json");
    let Some(contents) = read_optional_credential_file(&hosts_path)? else {
        return Ok(None);
    };
    parse_single_copilot_token(&contents).map(Some)
}

fn read_optional_credential_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !metadata.file_type().is_file() {
        return Err(());
    }
    read_limited(path).map(Some).ok_or(())
}

fn parse_single_copilot_token(contents: &[u8]) -> Result<String, ()> {
    let entries: std::collections::BTreeMap<String, CopilotTokenEntry> =
        serde_json::from_slice(contents).map_err(|_| ())?;
    if entries.len() != 1 {
        return Err(());
    }
    entries
        .into_values()
        .next()
        .and_then(|entry| entry.oauth_token)
        .filter(|token| !token.is_empty())
        .ok_or(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopilotTokenEntry {
    #[serde(rename = "oauth_token")]
    oauth_token: Option<String>,
    #[serde(rename = "user")]
    _user: Option<String>,
}

#[cfg(windows)]
fn usage_home_mismatch_requires_go() -> bool {
    let rust_home = home();
    let go_home = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    rust_home != go_home
}

#[cfg(not(windows))]
fn usage_home_mismatch_requires_go() -> bool {
    false
}

fn path_may_exist(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}

/// `$KIMI_CODE_HOME`, else the current `~/.kimi-code`, else the legacy
/// `~/.kimi` when that one is the only install with a credential file.
fn kimi_cli_home() -> PathBuf {
    if let Some(name) = env::var_os("KIMI_CODE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(name);
    }
    let current = home().join(".kimi-code");
    if current.join("credentials/kimi-code.json").exists() {
        return current;
    }
    let legacy = home().join(".kimi");
    if legacy.join("credentials/kimi-code.json").exists() {
        return legacy;
    }
    current
}

/// The CLI's stored access token and device id.
fn kimi_store(cli_home: &Path) -> (Option<String>, Option<String>) {
    let token = json_string(
        &cli_home.join("credentials/kimi-code.json"),
        &["access_token"],
    );
    let device_id = read_limited(&cli_home.join("device_id"))
        .and_then(|data| String::from_utf8(data).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    (token, device_id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KimiCredentialCandidate {
    #[serde(rename = "access_token")]
    access_token: Option<String>,
    #[serde(rename = "refresh_token")]
    _refresh_token: Option<String>,
}

/// Return only the deterministic subset of the Go Kimi file parser: the
/// canonical field spelling, without duplicate JSON keys, case aliases,
/// unknown fields, or secret refs.
fn kimi_file_token_candidate(path: &Path) -> Option<String> {
    let data = read_limited(path)?;
    let candidate: KimiCredentialCandidate = serde_json::from_slice(&data).ok()?;
    candidate
        .access_token
        .filter(|token| !token.is_empty() && !is_secret_reference(token))
}

/// The Go Kimi provider converts the device-id file's raw bytes to a string,
/// while Rust reads UTF-8. Keep existing non-ASCII or unreadable forms on Go
/// until their header encoding has source-bound parity evidence.
fn kimi_device_id_is_native(path: &Path) -> bool {
    if !path_may_exist(path) {
        return true;
    }
    read_limited(path).is_some_and(|bytes| {
        std::str::from_utf8(&bytes).is_ok_and(|value| {
            value
                .trim()
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        })
    })
}

fn nous_auth_path() -> PathBuf {
    env_path("HERMES_HOME", home().join(".hermes")).join("auth.json")
}

/// The `providers[]` entry with `id == "nous"`: the scoped `invoke_jwt` first,
/// then `access_token`. A JWT-shaped token must still be live; a plain token
/// passes through.
fn nous_file_token(path: &Path) -> Option<String> {
    let root = json_value(path)?;
    let providers = root.get("providers")?.as_array()?;
    for provider in providers {
        if provider.get("id").and_then(Value::as_str) != Some("nous") {
            continue;
        }
        let token = ["invoke_jwt", "access_token"].iter().find_map(|key| {
            provider
                .get(*key)
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
        })?;
        if token.contains('.') && !nous_jwt_is_live(token) {
            return None;
        }
        return Some(token.to_owned());
    }
    None
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NousCredentialCandidate {
    #[serde(rename = "version")]
    _version: Option<Value>,
    providers: Option<Vec<NousProviderCandidate>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NousProviderCandidate {
    id: Option<String>,
    invoke_jwt: Option<String>,
    access_token: Option<String>,
    #[serde(rename = "client_id")]
    _client_id: Option<Value>,
}

/// The Hermes file is native only when Go's typed decode has a deterministic
/// result. JWT expiry follows Go's float64-to-int64 Unix-second truncation.
fn nous_file_token_candidate(path: &Path) -> Option<String> {
    let data = read_limited(path)?;
    let root: NousCredentialCandidate = serde_json::from_slice(&data).ok()?;
    let provider = root
        .providers?
        .into_iter()
        .find(|provider| provider.id.as_deref() == Some("nous"))?;
    let token = provider
        .invoke_jwt
        .filter(|token| !token.is_empty())
        .or_else(|| provider.access_token.filter(|token| !token.is_empty()))?;
    if (token.contains('.') && !nous_jwt_is_live(&token)) || is_secret_reference(&token) {
        return None;
    }
    Some(token)
}

fn supported_custom_base(raw: &str) -> bool {
    if !trusted_https_url(raw, false) || !raw.is_ascii() || raw.contains(['?', '#', '%', '@']) {
        return false;
    }
    let Some(authority_and_path) = raw.strip_prefix("https://") else {
        return false;
    };
    let (authority, path) = authority_and_path
        .split_once('/')
        .unwrap_or((authority_and_path, ""));
    if !authority.contains('.')
        || !authority.split('.').all(|label| {
            !label.is_empty()
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return false;
    }
    path.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        && !path.contains("//")
        && !path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

fn supported_opencode_workspace(raw: &str) -> bool {
    raw.strip_prefix("wrk_").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

#[allow(clippy::cast_precision_loss)] // Mirrors Go's float64 expiry-to-int64 conversion and current Unix-second comparison.
fn nous_jwt_is_live(token: &str) -> bool {
    let mut parts = token.split('.');
    let (Some(_), Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some(expiry) = decode_base64url(payload)
        .and_then(|decoded| serde_json::from_slice::<JwtExpiryClaims>(&decoded).ok())
    else {
        return false;
    };
    let Some(expiry) = expiry.0 else {
        return false;
    };
    // Go decodes exp into float64, converts it to int64 (truncating toward
    // zero), then constructs a whole-second time.Time. Keep out-of-range
    // values on Go rather than relying on Rust's saturating float cast.
    if !expiry.is_finite() || expiry < i64::MIN as f64 || expiry >= i64::MAX as f64 {
        return false;
    }
    expiry.trunc() > seconds_since_epoch() as f64
}

struct JwtExpiryClaims(Option<f64>);

impl<'de> Deserialize<'de> for JwtExpiryClaims {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ClaimsVisitor;

        impl<'de> Visitor<'de> for ClaimsVisitor {
            type Value = JwtExpiryClaims;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JWT claims object with one exact exp field")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut expiry = None;
                let mut saw_exp = false;
                while let Some(key) = map.next_key::<String>()? {
                    if key.eq_ignore_ascii_case("exp") {
                        if key != "exp" || saw_exp {
                            let _: IgnoredAny = map.next_value()?;
                            return Err(serde::de::Error::custom(
                                "ambiguous or duplicate JWT expiry claim",
                            ));
                        }
                        saw_exp = true;
                        expiry = Some(map.next_value::<f64>()?);
                    } else {
                        let _: IgnoredAny = map.next_value()?;
                    }
                }
                Ok(JwtExpiryClaims(expiry))
            }
        }

        deserializer.deserialize_map(ClaimsVisitor)
    }
}

fn seconds_since_epoch() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Decodes an unpadded base64url payload. Rejecting noncanonical trailing
/// bits keeps this gate within a conservative subset of Go `RawURLEncoding`.
///
/// ponytail: hand-rolled because this one JWT claim decode is the crate's only
/// call site; switch to the `base64` crate (already pinned in guard-core) once a
/// second call site appears.
fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    let mut decoded = Vec::new();
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push(u8::try_from(buffer >> bits).ok()?);
            buffer &= (1 << bits) - 1;
        }
    }
    (bits != 6 && buffer == 0).then_some(decoded)
}

// ---------------------------------------------------------------------------
// Claude keychain (macOS)
// ---------------------------------------------------------------------------

/// Reads the Claude Code OAuth token from the macOS login keychain, with the
/// expiry the entry declares. Empty when nothing usable is stored — not being
/// signed in is a normal state, not an error.
#[cfg(target_os = "macos")]
fn claude_keychain_credential() -> Option<(String, Option<SystemTime>)> {
    if let Some(found) = claude_keychain_service_credential(CLAUDE_KEYCHAIN_SERVICE) {
        return Some(found);
    }
    // Only an install that does not use the bare name costs a keychain listing.
    for service in suffixed_claude_keychain_services() {
        if let Some(found) = claude_keychain_service_credential(&service) {
            return Some(found);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn claude_keychain_credential() -> Option<(String, Option<SystemTime>)> {
    None
}

#[cfg(target_os = "macos")]
fn claude_keychain_service_credential(service: &str) -> Option<(String, Option<SystemTime>)> {
    // mcpOAuth-only entries hold tokens for MCP server logins, not for the
    // Claude subscription endpoint, and count as "not signed in".
    let blob = bounded_command_stdout(
        "/usr/bin/security",
        &["find-generic-password", "-w", "-s", service],
        CLAUDE_KEYCHAIN_TIMEOUT,
        MAX_CREDENTIAL_FILE_BYTES,
    )?;
    parse_claude_keychain_blob(&blob)
}

#[derive(Default)]
struct ClaudeKeychainBlob {
    oauth: Option<ClaudeKeychainOAuth>,
}

#[derive(Default)]
struct ClaudeKeychainOAuth {
    access_token: String,
    expires_at_millis: Option<i64>,
}

#[derive(Default)]
struct ClaudeKeychainOAuthPatch {
    access_token: Option<String>,
    expires_at_millis: ExpiryPatch,
}

#[derive(Default)]
enum ExpiryPatch {
    #[default]
    Unchanged,
    Clear,
    Set(i64),
}

impl ClaudeKeychainOAuthPatch {
    fn apply(self, target: &mut ClaudeKeychainOAuth) {
        if let Some(access_token) = self.access_token {
            target.access_token = access_token;
        }
        match self.expires_at_millis {
            ExpiryPatch::Unchanged => {}
            ExpiryPatch::Clear => target.expires_at_millis = None,
            ExpiryPatch::Set(expires_at_millis) => {
                target.expires_at_millis = Some(expires_at_millis);
            }
        }
    }
}

impl<'de> Deserialize<'de> for ClaudeKeychainBlob {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BlobVisitor;

        impl<'de> Visitor<'de> for BlobVisitor {
            type Value = ClaudeKeychainBlob;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Claude keychain JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut blob = ClaudeKeychainBlob::default();
                while let Some(name) = map.next_key::<String>()? {
                    if go_json_field_matches(&name, "claudeAiOauth") {
                        match map.next_value::<Option<ClaudeKeychainOAuthPatch>>()? {
                            None => blob.oauth = None,
                            Some(patch) => {
                                patch.apply(
                                    blob.oauth.get_or_insert_with(ClaudeKeychainOAuth::default),
                                );
                            }
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(blob)
            }
        }

        deserializer.deserialize_map(BlobVisitor)
    }
}

impl<'de> Deserialize<'de> for ClaudeKeychainOAuthPatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OAuthPatchVisitor;

        impl<'de> Visitor<'de> for OAuthPatchVisitor {
            type Value = ClaudeKeychainOAuthPatch;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Claude keychain OAuth object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut patch = ClaudeKeychainOAuthPatch::default();
                while let Some(name) = map.next_key::<String>()? {
                    if go_json_field_matches(&name, "accessToken") {
                        // Go decoding null into a non-pointer string leaves
                        // the previous value untouched; malformed types fail.
                        if let Some(value) = map.next_value::<Option<String>>()? {
                            patch.access_token = Some(value);
                        }
                    } else if go_json_field_matches(&name, "expiresAt") {
                        // expiresAt is a pointer: null explicitly clears it.
                        patch.expires_at_millis = match map.next_value::<Option<i64>>()? {
                            Some(value) => ExpiryPatch::Set(value),
                            None => ExpiryPatch::Clear,
                        };
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(patch)
            }
        }

        deserializer.deserialize_map(OAuthPatchVisitor)
    }
}

fn go_json_field_matches(actual: &str, expected: &str) -> bool {
    fn fold(character: char) -> char {
        match character {
            // unicode.SimpleFold cycles these compatibility characters with
            // ASCII S/s and K/k; encoding/json's tagged-field matcher accepts
            // them even though Rust's eq_ignore_ascii_case does not.
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            character => character.to_ascii_lowercase(),
        }
    }

    actual.chars().map(fold).eq(expected.chars().map(fold))
}

/// Parses the same typed fields used by Go's Claude keychain decoder. Custom
/// map visitors preserve encoding/json's case-insensitive tagged-field match,
/// duplicate-object merge, scalar-null no-op, and pointer-null reset rules.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_claude_keychain_blob(blob: &[u8]) -> Option<(String, Option<SystemTime>)> {
    let text = go_json_compatible_text(blob);
    let text = text.trim();
    let decoded: ClaudeKeychainBlob = serde_json::from_str(text).ok()?;
    let oauth = decoded.oauth?;
    if oauth.access_token.is_empty() {
        return None;
    }
    let expires_at = oauth
        .expires_at_millis
        .filter(|milliseconds| *milliseconds > 0)
        .and_then(|milliseconds| {
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_millis(u64::try_from(milliseconds).ok()?))
        });
    Some((oauth.access_token, expires_at))
}

fn go_json_compatible_text(blob: &[u8]) -> String {
    // encoding/json decodes invalid UTF-8 with utf8.DecodeRune, replacing one
    // invalid byte at a time. Rust's from_utf8_lossy groups some invalid
    // prefixes, so preserve Go's bytewise replacement explicitly.
    let mut utf8 = String::with_capacity(blob.len());
    let mut remaining = blob;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                utf8.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                utf8.push_str(
                    std::str::from_utf8(&remaining[..valid_end])
                        .expect("valid_up_to marks a UTF-8 boundary"),
                );
                utf8.push('\u{fffd}');
                remaining = &remaining[valid_end + 1..];
            }
        }
    }

    // Go's JSON decoder replaces unpaired UTF-16 surrogate escapes with
    // U+FFFD; serde_json rejects them. Normalize only string escapes, leaving
    // malformed JSON escapes for serde_json to reject as before.
    let bytes = utf8.as_bytes();
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if !in_string {
            normalized.push(byte);
            index += 1;
            if byte == b'"' {
                in_string = true;
            }
            continue;
        }
        if byte == b'"' {
            normalized.push(byte);
            index += 1;
            in_string = false;
            continue;
        }
        if byte != b'\\' {
            normalized.push(byte);
            index += 1;
            continue;
        }

        if let Some(first) = unicode_escape_unit(bytes, index) {
            if (0xd800..=0xdbff).contains(&first) {
                if let Some(second) = unicode_escape_unit(bytes, index + 6)
                    && (0xdc00..=0xdfff).contains(&second)
                {
                    normalized.extend_from_slice(&bytes[index..index + 12]);
                    index += 12;
                    continue;
                }
                normalized.extend_from_slice(b"\\uFFFD");
                index += 6;
                continue;
            }
            if (0xdc00..=0xdfff).contains(&first) {
                normalized.extend_from_slice(b"\\uFFFD");
                index += 6;
                continue;
            }
            normalized.extend_from_slice(&bytes[index..index + 6]);
            index += 6;
            continue;
        }

        normalized.push(byte);
        index += 1;
        if index < bytes.len() {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(normalized).expect("normalized JSON text remains valid UTF-8")
}

fn unicode_escape_unit(bytes: &[u8], start: usize) -> Option<u16> {
    let escape = bytes.get(start..start.checked_add(6)?)?;
    if escape[0] != b'\\' || escape[1] != b'u' {
        return None;
    }
    let mut value = 0u16;
    for digit in &escape[2..] {
        value = value.checked_mul(16)? + u16::from(hex_digit(*digit)?);
    }
    Some(value)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Per-installation service names present in the keychain, in stable order.
/// `dump-keychain` lists item attributes only — no secrets, no approval panel.
#[cfg(target_os = "macos")]
fn suffixed_claude_keychain_services() -> Vec<String> {
    let prefix = format!("{CLAUDE_KEYCHAIN_SERVICE}-");
    let mut services: Vec<String> = claude_keychain_dump_names()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .filter(|name| valid_claude_service_name(name))
        .collect();
    services.sort();
    services.dedup();
    services
}

#[cfg(target_os = "macos")]
fn claude_keychain_dump_names() -> Vec<String> {
    let Some(output) = bounded_command_stdout(
        "/usr/bin/security",
        &["dump-keychain"],
        CLAUDE_KEYCHAIN_TIMEOUT,
        MAX_PROBE_OUTPUT_BYTES,
    ) else {
        return Vec::new();
    };
    names_from_keychain_dump(&String::from_utf8_lossy(&output))
}

/// Pulls the service out of a `dump-keychain` attribute line:
/// `    "svce"<blob>="Claude Code-credentials-552ffa86"`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn names_from_keychain_dump(dump: &str) -> Vec<String> {
    const MARKER: &str = "\"svce\"<blob>=\"";
    dump.lines()
        .filter_map(|line| {
            let start = line.find(MARKER)? + MARKER.len();
            let rest = &line[start..];
            let end = rest.rfind('"')?;
            (end > 0).then(|| rest[..end].to_owned())
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn valid_claude_service_name(name: &str) -> bool {
    if name == CLAUDE_KEYCHAIN_SERVICE {
        return true;
    }
    let Some(suffix) = name.strip_prefix(&format!("{CLAUDE_KEYCHAIN_SERVICE}-")) else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Antigravity process probe
// ---------------------------------------------------------------------------

fn antigravity_process_list_running(process_list: &str) -> bool {
    process_list.contains("agy") || process_list.contains("Antigravity")
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

fn auth_error(detail: &str) -> AuthStatus {
    AuthStatus {
        status: "missing".into(),
        detail: format!("credential resolution failed: {detail}"),
        source: Some("vault".into()),
    }
}

fn missing(detail: &str) -> AuthStatus {
    AuthStatus {
        status: "missing".into(),
        detail: detail.into(),
        source: None,
    }
}

fn available(detail: &str, source: String) -> AuthStatus {
    AuthStatus {
        status: "available".into(),
        detail: detail.into(),
        source: Some(source),
    }
}

#[must_use]
pub fn all_providers() -> Vec<Provider> {
    vec![
        claude(),
        codex(),
        copilot(),
        cursor(),
        kimi(),
        moonshot(),
        nous(),
        opencode(),
        openrouter(),
        antigravity(),
    ]
}

/// Whether the report would read a stored credential outside the native
/// provider subset, i.e. whether the Go fallback is required.
///
/// The checks here are prompt-free: environment values, credential files, and
/// the keychain *listing* (attributes only, never a secret value). The local
/// Antigravity probe is performed by its native provider. File-backed routes
/// and home overrides require a deterministic
/// Go-equivalent subset; secret references and unsupported base/workspace
/// overrides stay on Go.
/// Multiple configured providers can be native together when each source is
/// proven. A workspace override alone supplies no `OpenCode` cookie or
/// strategy, so it cannot start a fetch.
#[must_use]
pub fn needs_go_fallback() -> bool {
    let claude_admin_env = env_raw("ANTHROPIC_ADMIN_KEY");
    let claude_oauth_env = env_raw("ANTHROPIC_OAUTH_TOKEN");
    let copilot_env = env_raw("COPILOT_ACCESS_TOKEN");
    let openrouter_env = env_raw("OPENROUTER_API_KEY");
    let moonshot_env = env_raw("MOONSHOT_API_KEY");
    let cursor_env = env_raw("CURSOR_COOKIE");
    let kimi_api_env = env_raw("KIMI_CODE_API_KEY");
    let kimi_auth_env = env_raw("KIMI_AUTH_TOKEN");
    let nous_env = env_raw("NOUS_PORTAL_ACCESS_TOKEN");
    let codex_env = env_raw("CODEX_ACCESS_TOKEN");
    let opencode_env = env_raw("OPENCODE_COOKIE");
    let opencode_workspace_override = env_raw("OPENCODE_WORKSPACE_ID");
    let unsupported_base = [
        env_raw("KIMI_CODE_BASE_URL"),
        env_raw("HERMES_PORTAL_BASE_URL"),
        env_raw("OPENROUTER_API_URL"),
    ]
    .into_iter()
    .flatten()
    .any(|value| !supported_custom_base(&value));
    // Go resolves its home from USERPROFILE on Windows; Rust currently uses
    // HOME. Keep the entire usage report on Go if those roots differ so no
    // provider can silently miss a file-backed credential.
    if usage_home_mismatch_requires_go() {
        return true;
    }
    if unsupported_base {
        return true;
    }
    let Ok(copilot_file) = copilot_file_token_candidate_in(&copilot_config_dir()) else {
        return true;
    };
    let claude_file_path = home().join(".claude/.credentials.json");
    let claude_file_token = claude_file_token_in(&claude_file_path);
    if claude_file_token.is_none() && claude_file_path.exists() {
        // Go accepts case-insensitive struct tags and duplicate-map merge
        // semantics. If this strict candidate parser cannot prove equivalence,
        // let Go interpret the existing file before any keychain probe.
        return true;
    }
    let codex_file = codex_file_token(&codex_home());
    let nous_path = nous_auth_path();
    let nous_file = nous_file_token_candidate(&nous_path);
    if path_may_exist(&nous_path) && nous_file.is_none() {
        // Existing files that fall outside the deterministic plain-token
        // subset remain interpreted by Go, including expired JWTs.
        return true;
    }
    let kimi_path = kimi_cli_home().join("credentials/kimi-code.json");
    let kimi_cli = kimi_file_token_candidate(&kimi_path);
    if path_may_exist(&kimi_path) && kimi_cli.is_none() {
        return true;
    }
    if !kimi_device_id_is_native(&kimi_cli_home().join("device_id")) {
        return true;
    }
    // Without a cookie Go has no request strategy, so the workspace is only
    // reported as configured and its spelling cannot affect an HTTP request.
    if opencode_env.is_some()
        && opencode_workspace_override
            .as_deref()
            .is_some_and(|workspace| !supported_opencode_workspace(workspace))
    {
        return true;
    }
    needs_go_fallback_for(&UsageFallbackSignals {
        claude_admin_env: claude_admin_env.as_deref(),
        claude_oauth_env: claude_oauth_env.as_deref(),
        copilot_env: copilot_env.as_deref(),
        copilot_file: copilot_file.as_deref(),
        openrouter_env: openrouter_env.as_deref(),
        moonshot_env: moonshot_env.as_deref(),
        cursor_env: cursor_env.as_deref(),
        kimi_api_env: kimi_api_env.as_deref(),
        kimi_auth_env: kimi_auth_env.as_deref(),
        kimi_cli: kimi_cli.as_deref(),
        nous_env: nous_env.as_deref(),
        nous_file: nous_file.as_deref(),
        codex_env: codex_env.as_deref(),
        codex_file: codex_file.as_deref(),
        opencode_env: opencode_env.as_deref(),
        claude_file: claude_file_token.as_deref(),
        other_provider_env: false,
        other_credential_source: false,
        // Go skips its Keychain read when either the OAuth environment source
        // is present (even if resolving it fails) or the file supplied a
        // token. Avoid listing Keychain attributes in either case.
        local_provider_present: claude_oauth_env.is_none()
            && claude_file_token.is_none()
            && claude_keychain_present(),
    })
}

/// Keeps reports native for the proven portable sources once every configured
/// source is handled by the same provider constructors. Unsupported files,
/// secret references and the Claude Keychain source still keep Go in charge.
#[derive(Clone, Copy, Default)]
struct UsageFallbackSignals<'a> {
    claude_admin_env: Option<&'a str>,
    claude_oauth_env: Option<&'a str>,
    copilot_env: Option<&'a str>,
    copilot_file: Option<&'a str>,
    openrouter_env: Option<&'a str>,
    moonshot_env: Option<&'a str>,
    cursor_env: Option<&'a str>,
    kimi_api_env: Option<&'a str>,
    kimi_auth_env: Option<&'a str>,
    kimi_cli: Option<&'a str>,
    nous_env: Option<&'a str>,
    nous_file: Option<&'a str>,
    codex_env: Option<&'a str>,
    codex_file: Option<&'a str>,
    opencode_env: Option<&'a str>,
    claude_file: Option<&'a str>,
    other_provider_env: bool,
    other_credential_source: bool,
    local_provider_present: bool,
}

fn needs_go_fallback_for(signals: &UsageFallbackSignals<'_>) -> bool {
    let credentials = [
        signals.claude_admin_env,
        signals.claude_oauth_env,
        signals.copilot_env,
        signals.copilot_file,
        signals.openrouter_env,
        signals.moonshot_env,
        signals.cursor_env,
        signals.kimi_api_env,
        signals.kimi_auth_env,
        signals.kimi_cli,
        signals.nous_env,
        signals.nous_file,
        signals.codex_env,
        signals.codex_file,
        signals.opencode_env,
        signals.claude_file,
    ];
    if signals.other_provider_env
        || signals.other_credential_source
        || (signals.local_provider_present
            && signals.claude_oauth_env.is_none()
            && signals.claude_file.is_none())
    {
        return true;
    }
    credentials.into_iter().flatten().any(is_secret_reference)
}

/// Whether any Claude Code keychain service name exists, bare or suffixed.
///
/// Only attributes are listed — no secret is read, so deciding whether the
/// report stays on the shipped implementation cannot raise an approval panel.
#[cfg(target_os = "macos")]
fn claude_keychain_present() -> bool {
    claude_keychain_dump_names()
        .iter()
        .any(|name| valid_claude_service_name(name))
}

#[cfg(not(target_os = "macos"))]
fn claude_keychain_present() -> bool {
    false
}

fn claude() -> Provider {
    let admin = resolve_env("ANTHROPIC_ADMIN_KEY");
    let oauth = resolve_env_or_file("ANTHROPIC_OAUTH_TOKEN", claude_file_token);
    let (admin_value, admin_error) = match admin {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let (mut oauth_value, oauth_error) = match oauth {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let mut oauth_expires_at = None;
    // Only tried when neither the env var nor the file produced a token: the
    // keychain read can raise an approval panel.
    if oauth_value.is_none()
        && oauth_error.is_none()
        && let Some((token, expires_at)) = claude_keychain_credential()
    {
        oauth_value = Some(("keychain".into(), token));
        oauth_expires_at = expires_at;
    }
    claude_from_resolved(
        admin_value,
        admin_error,
        oauth_value,
        oauth_error,
        oauth_expires_at,
    )
}

pub(crate) fn claude_from_resolved(
    admin_value: Option<(String, String)>,
    admin_error: Option<String>,
    oauth_value: Option<(String, String)>,
    oauth_error: Option<String>,
    oauth_expires_at: Option<SystemTime>,
) -> Provider {
    let mut credentials = Vec::new();
    if let Some((_, value)) = admin_value.clone() {
        // The Go strategy's request/report source is always "api"; the
        // resolved source (env, vault, and so on) belongs in AuthStatus only.
        credentials.push(("api".into(), value));
    }
    if let Some((_credential_source, value)) = oauth_value.clone() {
        // Go's Claude OAuth strategy always reports `oauth` for the fetched
        // snapshot, while AuthStatus separately retains env/file/keychain
        // provenance. Keep those two source labels distinct.
        credentials.push(("oauth".into(), value));
    }
    let mut provider = Provider::new(
        "claude",
        "Claude",
        vec![
            "ANTHROPIC_ADMIN_KEY",
            "ANTHROPIC_OAUTH_TOKEN",
            "file",
            "keychain",
        ],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = admin_value.is_some() || oauth_value.is_some();
    provider.auth_status = if !provider.configured {
        match (admin_error, oauth_error) {
            (Some(error), _) | (None, Some(error)) => auth_error(&error),
            (None, None) => missing(
                "No Claude credentials found — add an admin key or sign in with the Claude CLI",
            ),
        }
    } else if let Some((source, _)) = oauth_value {
        if oauth_expires_at.is_some_and(|expiry| expiry < SystemTime::now()) {
            AuthStatus {
                status: "expired".into(),
                detail: "Claude Code OAuth token expired — sign in again with the Claude CLI"
                    .into(),
                source: Some(source),
            }
        } else {
            available("Signed in via Claude Code OAuth token", source)
        }
    } else {
        let source = admin_value.map_or_else(|| "env".to_owned(), |(source, _)| source);
        available("Admin API key from ANTHROPIC_ADMIN_KEY", source)
    };
    provider
}

fn codex() -> Provider {
    let home_dir = codex_home();
    let resolved = resolve_env_or_file("CODEX_ACCESS_TOKEN", || codex_file_token(&home_dir));
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    codex_from_resolved(value, error, home_dir.join("auth.json").exists())
}

pub(crate) fn codex_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    auth_file_exists: bool,
) -> Provider {
    let mut provider = Provider::new(
        "codex",
        "Codex",
        vec!["CODEX_ACCESS_TOKEN", "auth.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via Codex CLI OAuth (CODEX_ACCESS_TOKEN or auth.json)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else if auth_file_exists {
        AuthStatus {
            status: "expired".into(),
            detail: "Codex auth file found but no valid token — re-auth with the Codex CLI".into(),
            source: Some("file".into()),
        }
    } else {
        missing("No Codex credentials found — sign in with the Codex CLI")
    };
    provider
}

fn copilot() -> Provider {
    let resolved = resolve_env_or_file("COPILOT_ACCESS_TOKEN", copilot_file_token);
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    copilot_from_resolved(value, error)
}

pub(crate) fn copilot_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
) -> Provider {
    let mut provider = Provider::new(
        "copilot",
        "GitHub Copilot",
        vec!["COPILOT_ACCESS_TOKEN", "apps.json", "hosts.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via GitHub Copilot (COPILOT_ACCESS_TOKEN or Copilot CLI)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else {
        missing("No Copilot token found — sign in with the Copilot CLI")
    };
    provider
}

fn cursor() -> Provider {
    let resolved = resolve_env("CURSOR_COOKIE");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    cursor_from_resolved(value, error)
}

pub(crate) fn cursor_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
) -> Provider {
    let mut provider = Provider::new(
        "cursor",
        "Cursor",
        vec!["CURSOR_COOKIE"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("Cookie configured (CURSOR_COOKIE)", source)
    } else {
        missing("No Cursor credentials found — set CURSOR_COOKIE or sign in to Cursor.app")
    };
    provider
}

fn kimi() -> Provider {
    let cli_home = kimi_cli_home();
    let (cli_token, device_id) = kimi_store(&cli_home);
    let api_key = resolve_env("KIMI_CODE_API_KEY");
    let auth_token = resolve_env("KIMI_AUTH_TOKEN");
    let (api_value, api_failure) = match api_key {
        Ok(found) => (found, None),
        Err(failure) => (None, Some(failure)),
    };
    let (auth_value, auth_failure) = match auth_token {
        Ok(found) => (found, None),
        Err(failure) => (None, Some(failure)),
    };
    let base_url = env::var("KIMI_CODE_BASE_URL").map_or_else(
        |_| "https://api.kimi.com".into(),
        |value| validated_base(&value, "https://api.kimi.com"),
    );
    kimi_from_resolved(
        api_value,
        api_failure,
        cli_token.as_deref(),
        auth_value,
        auth_failure,
        base_url,
        device_id,
    )
}

pub(crate) fn kimi_from_resolved(
    api_value: Option<(String, String)>,
    api_failure: Option<String>,
    cli_token: Option<&str>,
    auth_value: Option<(String, String)>,
    auth_failure: Option<String>,
    base_url: String,
    device_id: Option<String>,
) -> Provider {
    // Strategy order is API key, then CLI token, then web auth token.
    let mut credentials = Vec::new();
    if let Some((_, value)) = api_value.clone() {
        credentials.push(("api".into(), value));
    }
    if let Some(token) = cli_token {
        credentials.push(("cli".into(), token.into()));
    }
    if let Some((_, value)) = auth_value.clone() {
        credentials.push(("web".into(), value));
    }
    let mut provider = Provider::new(
        "kimi",
        "Kimi Code",
        vec!["KIMI_CODE_API_KEY", "cli", "KIMI_AUTH_TOKEN"],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = api_value.is_some() || cli_token.is_some() || auth_value.is_some();
    provider.auth_status = if !provider.configured {
        match (api_failure, auth_failure) {
            (Some(failure), _) | (None, Some(failure)) => auth_error(&failure),
            (None, None) => missing("No Kimi Code CLI credentials found"),
        }
    } else if cli_token.is_some() {
        available("Kimi Code CLI is signed in", "cli".into())
    } else if let Some((source, _)) = api_value {
        available("API key from KIMI_CODE_API_KEY", source)
    } else {
        let source = auth_value.map_or_else(|| "env".to_owned(), |(source, _)| source);
        available("Web auth token from KIMI_AUTH_TOKEN", source)
    };
    provider.base_url = Some(base_url);
    provider.device_id = device_id.filter(|value| !value.is_empty());
    provider
}

fn moonshot() -> Provider {
    let resolved = resolve_env("MOONSHOT_API_KEY");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let region = if env::var("MOONSHOT_REGION").ok().as_deref() == Some("cn") {
        "cn"
    } else {
        "ai"
    };
    moonshot_from_resolved(value, error, region)
}

pub(crate) fn moonshot_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    region: &str,
) -> Provider {
    let mut provider = Provider::new(
        "moonshot",
        "Moonshot",
        vec!["MOONSHOT_API_KEY"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("API key from MOONSHOT_API_KEY", source)
    } else {
        missing("no API key configured (MOONSHOT_API_KEY)")
    };
    provider.region = if region == "cn" { "cn" } else { "ai" }.into();
    provider
}

fn nous() -> Provider {
    let path = nous_auth_path();
    let resolved = resolve_env_or_file("NOUS_PORTAL_ACCESS_TOKEN", || nous_file_token(&path));
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let base_url = env::var("HERMES_PORTAL_BASE_URL").map_or_else(
        |_| "https://portal.nousresearch.com".into(),
        |value| validated_base(&value, "https://portal.nousresearch.com"),
    );
    nous_from_resolved(value, error, base_url)
}

pub(crate) fn nous_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    base_url: String,
) -> Provider {
    let mut provider = Provider::new(
        "nous",
        "Nous Portal",
        vec!["NOUS_PORTAL_ACCESS_TOKEN", "auth.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via Hermes CLI auth store (NOUS_PORTAL_ACCESS_TOKEN or file)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else {
        missing("No Nous Portal credentials found — sign in with the Hermes CLI")
    };
    provider.base_url = Some(base_url);
    provider
}

fn opencode() -> Provider {
    let resolved = resolve_env("OPENCODE_COOKIE");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let workspace = env_raw("OPENCODE_WORKSPACE_ID");
    opencode_from_resolved(value, error, workspace.as_deref())
}

pub(crate) fn opencode_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    workspace: Option<&str>,
) -> Provider {
    let mut credentials: Vec<(String, String)> = value.clone().into_iter().collect();
    if let Some(workspace) = workspace {
        credentials.push(("workspace".into(), workspace.into()));
    }
    let mut provider = Provider::new(
        "opencode",
        "OpenCode Go",
        vec!["OPENCODE_COOKIE", "OPENCODE_WORKSPACE_ID"],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = value.is_some() || workspace.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("Cookie configured (OPENCODE_COOKIE)", source)
    } else if workspace.is_some() {
        available("Workspace override set", "env".into())
    } else {
        missing("No OpenCode Go credentials found — set OPENCODE_COOKIE or run OpenCode Go")
    };
    provider
}

fn openrouter() -> Provider {
    let resolved = resolve_env("OPENROUTER_API_KEY");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let base_url = env::var("OPENROUTER_API_URL").map_or_else(
        |_| "https://openrouter.ai/api/v1".into(),
        |value| validated_base(&value, "https://openrouter.ai/api/v1"),
    );
    openrouter_from_resolved(value, error, base_url)
}

pub(crate) fn openrouter_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    base_url: String,
) -> Provider {
    let mut provider = Provider::new(
        "openrouter",
        "OpenRouter",
        vec!["OPENROUTER_API_KEY"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("API key from OPENROUTER_API_KEY", source)
    } else {
        missing("no API key configured (OPENROUTER_API_KEY)")
    };
    provider.base_url = Some(base_url);
    provider
}

pub(super) fn antigravity_from_process_list(process_list: Option<&str>) -> Provider {
    let running = process_list.is_some_and(antigravity_process_list_running);
    let mut provider = Provider::new(
        "antigravity",
        "Antigravity",
        vec!["local process probe"],
        Vec::new(),
        AuthStatus::default(),
    );
    provider.configured = true;
    provider.auth_status = if running {
        available("Antigravity is running", "local".into())
    } else {
        missing("Antigravity is not running — start the Antigravity app or agy CLI")
    };
    provider
}

fn antigravity() -> Provider {
    let cancellation = crate::Cancellation::with_timeout(Duration::from_secs(2));
    let process_list = super::provider_requests::command_output_with_cancel(
        &cancellation,
        "ps",
        &["-ax", "-o", "pid=,command="],
    );
    antigravity_from_process_list(process_list.as_deref())
}

#[cfg(test)]
#[path = "provider_config_tests.rs"]
mod tests;
