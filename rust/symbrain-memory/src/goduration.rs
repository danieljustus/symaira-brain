//! `time.ParseDuration` and the `max_age` wrapper of the memory service.

use crate::slog::quote_bytes;

fn invalid(original: &str) -> String {
    format!(
        "time: invalid duration {}",
        quote_bytes(original.as_bytes())
    )
}

/// `time.ParseDuration`: nanoseconds, or the shipped error text.
///
/// # Errors
/// Returns the Go diagnostic for a malformed, unit-less, unknown-unit or
/// overflowing duration.
#[allow(clippy::too_many_lines)]
pub(crate) fn parse(original: &str) -> Result<i64, String> {
    const LIMIT: u64 = 1 << 63;
    let mut rest = original;
    let mut negative = false;
    if let Some(first) = rest.chars().next()
        && (first == '-' || first == '+')
    {
        negative = first == '-';
        rest = &rest[1..];
    }
    if rest == "0" {
        return Ok(0);
    }
    if rest.is_empty() {
        return Err(invalid(original));
    }
    let mut total: u64 = 0;
    while !rest.is_empty() {
        let first = rest.as_bytes()[0];
        if !(first == b'.' || first.is_ascii_digit()) {
            return Err(invalid(original));
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let mut whole: u64 = 0;
        for byte in rest.bytes().take(digits) {
            whole = whole
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                .filter(|value| *value <= LIMIT)
                .ok_or_else(|| invalid(original))?;
        }
        let pre = digits > 0;
        rest = &rest[digits..];
        let (mut fraction, mut scale) = (0_u64, 1.0_f64);
        let mut post = false;
        if let Some(tail) = rest.strip_prefix('.') {
            let count = tail.bytes().take_while(u8::is_ascii_digit).count();
            let mut overflow = false;
            for byte in tail.bytes().take(count) {
                if overflow {
                    continue;
                }
                match fraction
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                    .filter(|value| *value <= LIMIT)
                {
                    Some(value) => {
                        fraction = value;
                        scale *= 10.0;
                    }
                    None => overflow = true,
                }
            }
            post = count > 0;
            rest = &tail[count..];
        }
        if !pre && !post {
            return Err(invalid(original));
        }
        let unit_len = rest
            .bytes()
            .take_while(|byte| *byte != b'.' && !byte.is_ascii_digit())
            .count();
        if unit_len == 0 {
            return Err(format!(
                "time: missing unit in duration {}",
                quote_bytes(original.as_bytes())
            ));
        }
        let name = &rest[..unit_len];
        rest = &rest[unit_len..];
        let unit: u64 = match name {
            "ns" => 1,
            "us" | "\u{b5}s" | "\u{3bc}s" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => {
                return Err(format!(
                    "time: unknown unit {} in duration {}",
                    quote_bytes(name.as_bytes()),
                    quote_bytes(original.as_bytes())
                ));
            }
        };
        if whole > LIMIT / unit {
            return Err(invalid(original));
        }
        whole *= unit;
        if fraction > 0 {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let extra = (fraction as f64 * (unit as f64 / scale)) as u64;
            whole = whole.checked_add(extra).ok_or_else(|| invalid(original))?;
            if whole > LIMIT {
                return Err(invalid(original));
            }
        }
        total = total.checked_add(whole).ok_or_else(|| invalid(original))?;
        if total > LIMIT {
            return Err(invalid(original));
        }
    }
    if negative {
        return Ok(i64::try_from(total).map_or(i64::MIN, |value| -value));
    }
    i64::try_from(total).map_err(|_| invalid(original))
}

/// The memory service's `parseDuration`: a `d` suffix counts days.
///
/// # Errors
/// Returns the shipped diagnostic.
pub(crate) fn parse_max_age(text: &str) -> Result<i64, String> {
    if text.is_empty() {
        return Ok(0);
    }
    if text.len() < 2 {
        return Err(format!("invalid duration: {text}"));
    }
    if let Some(days) = text.strip_suffix('d') {
        let count: i64 = days.parse().map_err(|error: std::num::ParseIntError| {
            let reason = match error.kind() {
                std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                    "value out of range"
                }
                _ => "invalid syntax",
            };
            format!(
                "strconv.Atoi: parsing {}: {reason}",
                quote_bytes(days.as_bytes())
            )
        })?;
        return Ok(count.wrapping_mul(24).wrapping_mul(3_600_000_000_000));
    }
    parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_time_parse_duration() {
        assert_eq!(parse("1h30m"), Ok(5_400_000_000_000));
        assert_eq!(parse("1.5s"), Ok(1_500_000_000));
        assert_eq!(parse("-2m"), Ok(-120_000_000_000));
        assert_eq!(parse("0"), Ok(0));
        assert_eq!(parse("").unwrap_err(), "time: invalid duration \"\"");
        assert_eq!(
            parse("5").unwrap_err(),
            "time: missing unit in duration \"5\""
        );
        assert_eq!(
            parse("5x").unwrap_err(),
            "time: unknown unit \"x\" in duration \"5x\""
        );
        assert_eq!(parse("abc").unwrap_err(), "time: invalid duration \"abc\"");
        assert_eq!(parse_max_age("2d"), Ok(172_800_000_000_000));
        assert_eq!(
            parse_max_age("xd").unwrap_err(),
            "strconv.Atoi: parsing \"x\": invalid syntax"
        );
        assert_eq!(parse_max_age("x").unwrap_err(), "invalid duration: x");
    }
}
