//! Go `time.ParseDuration`.

/// Go `time.ParseDuration` in nanoseconds; `None` is any parse failure (the
/// flag package reports every failure as `parse error`).
pub(super) fn parse_duration(raw: &[u8]) -> Option<i64> {
    const LIMIT: u64 = 1 << 63;
    let mut rest = raw;
    let negative = matches!(rest.first(), Some(b'-'));
    if matches!(rest.first(), Some(b'-' | b'+')) {
        rest = &rest[1..];
    }
    if rest == b"0" {
        return Some(0);
    }
    if rest.is_empty() {
        return None;
    }
    let mut total = 0_u64;
    while !rest.is_empty() {
        if !(rest[0] == b'.' || rest[0].is_ascii_digit()) {
            return None;
        }
        let before = rest.len();
        let mut whole = 0_u64;
        while let Some(&digit) = rest.first().filter(|byte| byte.is_ascii_digit()) {
            if whole > LIMIT / 10 {
                return None;
            }
            whole = whole * 10 + u64::from(digit - b'0');
            if whole > LIMIT {
                return None;
            }
            rest = &rest[1..];
        }
        let pre = before != rest.len();
        let (mut fraction, mut scale, mut post) = (0_u64, 1_f64, false);
        if rest.first() == Some(&b'.') {
            rest = &rest[1..];
            let start = rest.len();
            let mut overflow = false;
            while let Some(&digit) = rest.first().filter(|byte| byte.is_ascii_digit()) {
                rest = &rest[1..];
                if overflow {
                    continue;
                }
                if fraction > (LIMIT - 1) / 10 {
                    overflow = true;
                    continue;
                }
                let next = fraction * 10 + u64::from(digit - b'0');
                if next > LIMIT {
                    overflow = true;
                    continue;
                }
                fraction = next;
                scale *= 10.0;
            }
            post = start != rest.len();
        }
        if !pre && !post {
            return None;
        }
        let unit_len = rest
            .iter()
            .position(|&byte| byte == b'.' || byte.is_ascii_digit())
            .unwrap_or(rest.len());
        if unit_len == 0 {
            return None;
        }
        let unit: u64 = match &rest[..unit_len] {
            b"ns" => 1,
            b"us" | b"\xc2\xb5s" | b"\xce\xbcs" => 1_000,
            b"ms" => 1_000_000,
            b"s" => 1_000_000_000,
            b"m" => 60_000_000_000,
            b"h" => 3_600_000_000_000,
            _ => return None,
        };
        rest = &rest[unit_len..];
        if whole > LIMIT / unit {
            return None;
        }
        let mut value = whole * unit;
        if fraction > 0 {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let extra = (fraction as f64 * (unit as f64 / scale)) as u64;
            value = value.saturating_add(extra);
            if value > LIMIT {
                return None;
            }
        }
        total = total.saturating_add(value);
        if total > LIMIT {
            return None;
        }
    }
    if negative {
        return Some(i64::try_from(-i128::from(total)).unwrap_or(i64::MIN));
    }
    i64::try_from(total).ok()
}
