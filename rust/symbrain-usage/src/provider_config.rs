//! Provider credential state machine and per-source auth texts.
//!
//! Mirrors `internal/usage/{claude,codex,copilot,cursor,kimi,moonshot,nous,
//! opencode,openrouter,antigravity}.go`, including the exact missing/expired/
//! available texts and the source tag each state reports. Resolution order per
//! provider is: environment variable (symvault/keychain capable), then the
//! provider's own credential file, then — Claude on macOS only — the login
//! keychain.

use super::provider_requests::validated_base;
use super::{AuthStatus, MAX_CREDENTIAL_FILE_BYTES, Provider, Value};
use std::env;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

/// Environment variables that configure a provider on their own.
const PROVIDER_ENV_VARS: &[&str] = &[
    "ANTHROPIC_ADMIN_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "CODEX_ACCESS_TOKEN",
    "COPILOT_ACCESS_TOKEN",
    "CURSOR_COOKIE",
    "KIMI_CODE_API_KEY",
    "KIMI_AUTH_TOKEN",
    "MOONSHOT_API_KEY",
    "NOUS_PORTAL_ACCESS_TOKEN",
    "OPENCODE_COOKIE",
    "OPENCODE_WORKSPACE_ID",
    "OPENROUTER_API_KEY",
];

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
/// Bound for the prompt-free process probes (`ps`, `lsof`).
const PROCESS_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
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

fn is_secret_reference(value: &str) -> bool {
    ["symvault://", "vault://", "env://", "keychain://"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
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
            .filter(|path| {
                !path.is_empty() && !path.starts_with('-') && !path.chars().any(char::is_control)
            })
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
    let output = bounded_command_stdout(
        command,
        args,
        Duration::from_secs(5),
        MAX_CREDENTIAL_FILE_BYTES,
    )?;
    String::from_utf8(output)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Runs one command with a hard deadline and a bounded stdout, never through a
/// shell. Output goes to a temporary file so a child holding the pipe open
/// cannot outlive cancellation.
fn bounded_command_stdout(
    command: &str,
    args: &[&str],
    timeout: Duration,
    cap: u64,
) -> Option<Vec<u8>> {
    let file = tempfile::NamedTempFile::new().ok()?;
    let handle = file.as_file().try_clone().ok()?;
    let mut child_command = Command::new(command);
    child_command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::from(handle));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut child_command, 0);
    let mut child = child_command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) | Err(_) => {
                terminate_child(&mut child);
                return None;
            }
        }
    }
    terminate_child(&mut child);
    let mut file = file.as_file().try_clone().ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut output = Vec::new();
    file.take(cap + 1).read_to_end(&mut output).ok()?;
    (output.len() as u64 <= cap).then_some(output)
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
    env::var_os(name).map_or(fallback, PathBuf::from)
}

// ---------------------------------------------------------------------------
// Provider credential files
// ---------------------------------------------------------------------------

/// `~/.claude/.credentials.json`, preferring the `default` account and then any
/// account carrying a token (file order, like the shipped map iteration).
fn claude_file_token() -> Option<String> {
    claude_file_token_in(&home().join(".claude/.credentials.json"))
}

fn claude_file_token_in(path: &Path) -> Option<String> {
    let root = json_value(path)?;
    let accounts = root.get("oauthAccount")?.as_object()?;
    if let Some(token) = accounts
        .get("default")
        .and_then(|account| account.get("accessToken"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
    {
        return Some(token.to_owned());
    }
    accounts
        .values()
        .find_map(|account| {
            account
                .get("accessToken")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
        })
        .map(Into::into)
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
    home().join(".config/github-copilot")
}

/// `apps.json`, then `hosts.json`: the `github.com:` host entry first, then any
/// entry with an `oauth_token`.
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

/// `$KIMI_CODE_HOME`, else the current `~/.kimi-code`, else the legacy
/// `~/.kimi` when that one is the only install with a credential file.
fn kimi_cli_home() -> PathBuf {
    if let Some(name) = env::var_os("KIMI_CODE_HOME") {
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

fn nous_jwt_is_live(token: &str) -> bool {
    let mut parts = token.split('.');
    let (Some(_), Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some(claims) = decode_base64url(payload)
        .and_then(|decoded| serde_json::from_slice::<Value>(&decoded).ok())
    else {
        return false;
    };
    let Some(expiry) = claims.get("exp").and_then(Value::as_f64) else {
        return false;
    };
    seconds_since_epoch() < expiry
}

fn seconds_since_epoch() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

/// Decodes an unpadded base64url payload.
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
            b'=' => continue,
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
    Some(decoded)
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
    let root: Value = serde_json::from_slice(blob.trim_ascii()).ok()?;
    let oauth = root.get("claudeAiOauth")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())?;
    let expires_at = oauth
        .get("expiresAt")
        .and_then(Value::as_i64)
        .filter(|milliseconds| *milliseconds > 0)
        .and_then(|milliseconds| {
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_millis(u64::try_from(milliseconds).ok()?))
        });
    Some((token.to_owned(), expires_at))
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

/// Whether the Antigravity app or `agy` CLI is running. The provider is always
/// "configured"; availability depends on the running language server.
fn antigravity_running() -> bool {
    let Some(output) = bounded_command_stdout(
        "ps",
        &["-ax", "-o", "pid=,command="],
        PROCESS_PROBE_TIMEOUT,
        MAX_PROBE_OUTPUT_BYTES,
    ) else {
        return false;
    };
    let list = String::from_utf8_lossy(&output);
    list.contains("agy") || list.contains("Antigravity")
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

/// Whether the report would read a stored credential or probe a running
/// Antigravity, i.e. whether a fetch against a live endpoint would run.
///
/// The checks here are prompt-free: environment values, credential files, the
/// keychain *listing* (attributes only, never a secret value), and the process
/// table. The CLI keeps such reports on the shipped implementation until the
/// provider fetch paths are pinned byte-for-byte.
#[must_use]
pub fn needs_go_fallback() -> bool {
    if PROVIDER_ENV_VARS.iter().any(|name| env_raw(name).is_some()) {
        return true;
    }
    if claude_file_token().is_some()
        || codex_file_token(&codex_home()).is_some()
        || copilot_file_token().is_some()
        || kimi_store(&kimi_cli_home()).0.is_some()
        || nous_file_token(&nous_auth_path()).is_some()
    {
        return true;
    }
    claude_keychain_present() || antigravity_running()
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
    let mut credentials = Vec::new();
    if let Some((source, value)) = admin_value.clone() {
        credentials.push((source, value));
    }
    if let Some((source, value)) = oauth_value.clone() {
        credentials.push((source, value));
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
    } else if home_dir.join("auth.json").exists() {
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
    // Strategy order is API key, then CLI token, then web auth token.
    let mut credentials = Vec::new();
    if let Some((source, value)) = api_value.clone() {
        credentials.push((source, value));
    }
    if let Some(token) = cli_token.clone() {
        credentials.push(("cli".into(), token));
    }
    if let Some((source, value)) = auth_value.clone() {
        credentials.push((source, value));
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
    provider.base_url = Some(env::var("KIMI_CODE_BASE_URL").map_or_else(
        |_| "https://api.kimi.com".into(),
        |value| validated_base(&value, "https://api.kimi.com"),
    ));
    provider.device_id = device_id.filter(|value| !value.is_empty());
    provider
}

fn moonshot() -> Provider {
    let resolved = resolve_env("MOONSHOT_API_KEY");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
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
    provider.region = if env::var("MOONSHOT_REGION").ok().as_deref() == Some("cn") {
        "cn".into()
    } else {
        "ai".into()
    };
    provider
}

fn nous() -> Provider {
    let path = nous_auth_path();
    let resolved = resolve_env_or_file("NOUS_PORTAL_ACCESS_TOKEN", || nous_file_token(&path));
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
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
    provider.base_url = Some(env::var("HERMES_PORTAL_BASE_URL").map_or_else(
        |_| "https://portal.nousresearch.com".into(),
        |value| validated_base(&value, "https://portal.nousresearch.com"),
    ));
    provider
}

fn opencode() -> Provider {
    let resolved = resolve_env("OPENCODE_COOKIE");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let workspace = env_raw("OPENCODE_WORKSPACE_ID");
    let mut credentials: Vec<(String, String)> = value.clone().into_iter().collect();
    if let Some(workspace) = workspace.clone() {
        credentials.push(("workspace".into(), workspace));
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
    provider.base_url = Some(env::var("OPENROUTER_API_URL").map_or_else(
        |_| "https://openrouter.ai/api/v1".into(),
        |value| validated_base(&value, "https://openrouter.ai/api/v1"),
    ));
    provider
}

fn antigravity() -> Provider {
    let running = antigravity_running();
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

#[cfg(test)]
#[path = "provider_config_tests.rs"]
mod tests;
