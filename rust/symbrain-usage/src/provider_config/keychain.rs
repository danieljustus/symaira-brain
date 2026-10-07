// Native provider keychain compatibility implementation.
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
