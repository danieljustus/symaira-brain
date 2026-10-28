/// `$KIMI_CODE_HOME`, else the current `~/.kimi-code`, else the legacy
/// `~/.kimi` when that one is the only install with a credential file.
// Native provider kimi nous compatibility implementation.
fn kimi_cli_home() -> PathBuf {
    if let Some(name) = env::var_os("KIMI_CODE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(name);
    }
    let current = credential_join(&home(), ".kimi-code");
    if credential_join(&current, "credentials/kimi-code.json").exists() {
        return current;
    }
    let legacy = credential_join(&home(), ".kimi");
    if credential_join(&legacy, "credentials/kimi-code.json").exists() {
        return legacy;
    }
    current
}

/// The CLI's stored access token, device id and optional expiry.
/// API keys and CLI OAuth currently share /coding/v1/usages; which API-key
/// products it accepts remains unproven offline (#931).
fn kimi_store(cli_home: &Path) -> (Option<String>, Option<String>, Option<f64>) {
    let (token, expiry) = kimi_file_credential(&credential_join(cli_home, "credentials/kimi-code.json"));
    let device_id = read_limited(&credential_join(cli_home, "device_id"))
        .and_then(|data| String::from_utf8(data).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    (token, device_id, expiry)
}

fn kimi_file_token_candidate(path: &Path) -> Option<String> {
    kimi_file_credential(path).0
}

#[derive(Deserialize)]
struct KimiExpiryMetadata {
    #[serde(default)]
    expires_at: serde_json::Value,
}

fn kimi_file_credential(path: &Path) -> (Option<String>, Option<f64>) {
    let Some(bytes) = read_provider_credentials(path) else { return (None, None) };
    let text = go_json_compatible_text(&bytes);
    let token = go_json_credential_limits(&text, false)
        .then(|| decode_typed_credential(&text, "access_token"))
        .flatten();
    let expiry = serde_json::from_str::<KimiExpiryMetadata>(&text).ok()
        .and_then(|metadata| kimi_expiry_seconds(&metadata.expires_at));
    (token, expiry)
}

// Unix seconds and RFC3339 metadata are advisory; unknown shapes still go to
// the endpoint. No refresh-token decoding, token rotation or file writes.
#[allow(clippy::cast_precision_loss)] // Subsecond precision is sufficient for expiry.
fn kimi_expiry_seconds(value: &serde_json::Value) -> Option<f64> {
    if let Some(text) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(text).ok()
            .map(|time| time.timestamp_millis() as f64 / 1000.0);
    }
    value.as_f64().filter(|seconds| (-62_135_596_800.0..253_402_300_800.0).contains(seconds))
}

// Invalid regular JSON is native missing credentials. Unsafe file access is
// native only where Go-derived evidence exists (#768, macOS). On Linux
// cap-std's openat2(RESOLVE_BENEATH|O_NOFOLLOW) rejects an in-root relative
// symlink that Go's os.Root follows, and Windows has no evidence yet, so those
// targets keep the Go route for unsafe shapes.
#[cfg(not(target_os = "macos"))]
fn kimi_file_requires_go(path: &Path) -> bool {
    read_optional_credential_file(path).is_err()
}

#[cfg(target_os = "macos")]
fn kimi_file_requires_go(_path: &Path) -> bool {
    false
}

/// Preserve Go's trimmed device bytes for the proven regular UTF-8 family.
/// Invalid UTF-8, internal ASCII controls and unsafe file access retain Go;
/// Unicode bytes require full request/report and actual wire evidence.
fn kimi_device_id_is_native(path: &Path) -> bool {
    match read_optional_credential_file(path) {
        Ok(None) => true,
        Ok(Some(bytes)) => std::str::from_utf8(&bytes).is_ok_and(|value| {
            value
                .trim()
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic() || byte >= 0x80)
        }),
        Err(()) => false,
    }
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
