//! Frozen Go `ParseInt` base-zero and overflow precedence.

pub(super) fn parse_integer(raw: &[u8]) -> Result<i64, &'static str> {
    let (negative, text) = match raw.first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let (radix, digits, prefix) = if let Some(d) = text
        .strip_prefix(b"0x")
        .or_else(|| text.strip_prefix(b"0X"))
    {
        (16, d, true)
    } else if let Some(d) = text
        .strip_prefix(b"0b")
        .or_else(|| text.strip_prefix(b"0B"))
    {
        (2, d, true)
    } else if let Some(d) = text
        .strip_prefix(b"0o")
        .or_else(|| text.strip_prefix(b"0O"))
    {
        (8, d, true)
    } else if text.starts_with(b"0") && text.len() > 1 {
        (8, &text[1..], true)
    } else {
        (10, text, false)
    };
    if digits.is_empty() {
        return Err("parse error");
    }
    let mut value = 0_u64;
    for &byte in digits {
        if byte == b'_' {
            continue;
        }
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'z' => byte - b'a' + 10,
            b'A'..=b'Z' => byte - b'A' + 10,
            _ => return Err("parse error"),
        };
        if u64::from(digit) >= radix {
            return Err("parse error");
        }
        value = value
            .checked_mul(radix)
            .and_then(|n| n.checked_add(u64::from(digit)))
            .ok_or("value out of range")?;
    }
    let mut last_digit = prefix;
    for &byte in digits {
        if byte == b'_' && !last_digit {
            return Err("parse error");
        }
        last_digit = byte != b'_';
    }
    if !last_digit {
        return Err("parse error");
    }
    let maximum = if negative {
        1_u64 << 63
    } else {
        i64::MAX.cast_unsigned()
    };
    if value > maximum {
        return Err("value out of range");
    }
    Ok(if negative {
        value.wrapping_neg().cast_signed()
    } else {
        value.cast_signed()
    })
}
