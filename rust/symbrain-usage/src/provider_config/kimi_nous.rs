/// `$KIMI_CODE_HOME`, else the current `~/.kimi-code`, else the legacy
/// `~/.kimi` when that one is the only install with a credential file.
// Native provider kimi nous compatibility implementation.
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
