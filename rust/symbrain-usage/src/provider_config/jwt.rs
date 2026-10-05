// Unverified JWT expiry is a read-only credential presence check, never auth.
#[allow(clippy::cast_precision_loss)]
fn jwt_expiry_out_of_range(expiry: f64) -> bool {
    !expiry.is_finite() || expiry < i64::MIN as f64 || expiry >= i64::MAX as f64
}

#[allow(clippy::cast_precision_loss)]
fn nous_jwt_is_live(token: &str) -> bool {
    jwt_expiry(token).is_some_and(|expiry| {
        !jwt_expiry_out_of_range(expiry) && expiry.trunc() > seconds_since_epoch() as f64
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
