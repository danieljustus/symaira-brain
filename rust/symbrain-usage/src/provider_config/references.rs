// Native provider references compatibility implementation.
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
