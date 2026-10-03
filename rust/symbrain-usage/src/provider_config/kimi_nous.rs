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
