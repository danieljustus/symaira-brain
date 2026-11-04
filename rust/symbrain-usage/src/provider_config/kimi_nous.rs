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
    let device_id = kimi_device_id_bytes(cli_home)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    (token, device_id, expiry)
}

/// The device id file trimmed the way Go's `strings.TrimSpace(string(data))`
/// trims it: whole Unicode white-space runes at both ends, invalid UTF-8 bytes
/// kept verbatim (they are not white space).
fn kimi_device_id_bytes(cli_home: &Path) -> Option<Vec<u8>> {
    let data = read_limited(&credential_join(cli_home, "device_id"))?;
    let trimmed = go_trim_space(&data);
    (!trimmed.is_empty()).then(|| trimmed.to_vec())
}

fn go_trim_space(mut bytes: &[u8]) -> &[u8] {
    let space = |character: char| character.is_whitespace();
    while let Some(first) = bytes.first() {
        let length = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        match bytes.get(..length).and_then(|head| std::str::from_utf8(head).ok()) {
            Some(head) if head.chars().next().is_some_and(space) => bytes = &bytes[length..],
            _ => break,
        }
    }
    while !bytes.is_empty() {
        let start = (bytes.len().saturating_sub(4)..bytes.len())
            .find(|start| {
                std::str::from_utf8(&bytes[*start..]).is_ok_and(|tail| tail.chars().count() == 1)
            });
        match start {
            Some(start)
                if std::str::from_utf8(&bytes[start..])
                    .ok()
                    .and_then(|tail| tail.chars().next())
                    .is_some_and(space) =>
            {
                bytes = &bytes[..start];
            }
            _ => break,
        }
    }
    bytes
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
