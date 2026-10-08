// Native provider environment compatibility implementation.
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
    let value = resolve_reference(&raw, "").map_err(|error| format!("resolve {name}: {error}"))?;
    Ok(Some((source.into(), value)))
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
