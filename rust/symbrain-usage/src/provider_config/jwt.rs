// Unverified JWT expiry is a read-only credential presence check, never auth.
//
// Go evaluates `time.Unix(int64(exp), 0).After(time.Now())`. The float-to-int
// conversion of an out-of-range value is architecture-specific (amd64 yields
// the minimum int64, arm64 saturates), but `time.Unix` adds the 62135596800s
// epoch offset with wrapping arithmetic, so every out-of-range value, and every
// value within that offset of the maximum, ends up before the present on both
// architectures. Rust's saturating cast plus the same wrapping addition is
// therefore exact for both.
const GO_UNIX_TO_INTERNAL_SECONDS: i64 = 62_135_596_800;

#[allow(clippy::cast_possible_truncation)]
fn nous_jwt_is_live(token: &str) -> bool {
    jwt_expiry(token).is_some_and(|expiry| {
        let expires = (expiry as i64).wrapping_add(GO_UNIX_TO_INTERNAL_SECONDS);
        let now = i64::try_from(seconds_since_epoch())
            .unwrap_or(i64::MAX)
            .wrapping_add(GO_UNIX_TO_INTERNAL_SECONDS);
        expires > now
    })
}

fn jwt_expiry(token: &str) -> Option<f64> {
    let mut parts = token.split('.');
    let (Some(_), Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let text = go_json_compatible_text(&decode_base64url(payload)?);
    if !go_json_credential_limits(&text, false) {
        return None;
    }
    serde_json::from_str::<JwtExpiryClaims>(&text).ok()?.0
}

struct JwtExpiryClaims(Option<f64>);
impl<'de> Deserialize<'de> for JwtExpiryClaims {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ClaimsVisitor;
        impl<'de> Visitor<'de> for ClaimsVisitor {
            type Value = JwtExpiryClaims;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JWT claims with typed float64 expiry")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut expiry = None;
                while let Some(key) = map.next_key::<String>()? {
                    if go_json_field_matches(&key, "exp") {
                        expiry = map.next_value::<Option<f64>>()?;
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

// Go RawURLEncoding ignores CR/LF and permits nonzero unused trailing bits.
// Padding, other whitespace and a one-character tail remain invalid.
fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    let mut decoded = Vec::new();
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'\r' | b'\n' => continue,
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
    (bits != 6).then_some(decoded)
}
