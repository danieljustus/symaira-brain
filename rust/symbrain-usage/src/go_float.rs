//! Go1.26.7 `ParseFloat` grammar and binary64 conversion dispatch.
// Hex scanning/rounding adapted from Go internal/strconv atof.go/atoi.go;
// Copyright 2009-2026 The Go Authors. BSD notice: migration/licenses/go-strconv-bsd.txt.

pub(super) fn parse(value: &str) -> Option<f64> {
    if value.eq_ignore_ascii_case("nan") {
        return Some(f64::from_bits(0x7ff8_0000_0000_0001));
    }
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    if unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity") {
        return Some(if value.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    let number = scan(value)?;
    if number.hex {
        hex_float(number)
    } else {
        super::go_decimal::parse(value, &number)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Number {
    pub(super) mantissa: u64,
    pub(super) exponent: i64,
    pub(super) negative: bool,
    pub(super) truncated: bool,
    hex: bool,
}

fn scan(value: &str) -> Option<Number> {
    let bytes = value.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let negative = bytes.first() == Some(&b'-');
    let hex = bytes
        .get(index..index + 2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"0x"));
    if hex {
        index += 2;
    }
    let base = if hex { 16 } else { 10 };
    let max_digits = if hex { 16 } else { 19 };
    let mut mantissa = 0_u64;
    let (mut digits, mut kept, mut point) = (0_i64, 0_i64, 0_i64);
    let (mut dot, mut saw_digit, mut truncated) = (false, false, false);
    while let Some(&byte) = bytes.get(index) {
        if byte == b'_' {
            index += 1;
            continue;
        }
        if byte == b'.' && !dot {
            dot = true;
            point = digits;
            index += 1;
            continue;
        }
        let Some(digit) = digit(byte, hex) else {
            break;
        };
        saw_digit = true;
        if digit == 0 && digits == 0 {
            point -= 1;
        } else {
            digits += 1;
            if kept < max_digits {
                mantissa = mantissa * base + u64::from(digit);
                kept += 1;
            } else if digit != 0 {
                truncated = true;
            }
        }
        index += 1;
    }
    if !saw_digit {
        return None;
    }
    if !dot {
        point = digits;
    }
    if hex {
        point *= 4;
        kept *= 4;
    }
    let (end, explicit_exponent) = exponent(bytes, index, hex)?;
    index = end;
    point += explicit_exponent;
    if index != bytes.len() || !underscores_valid(bytes) {
        return None;
    }
    Some(Number {
        mantissa,
        exponent: if mantissa == 0 { 0 } else { point - kept },
        negative,
        truncated,
        hex,
    })
}

pub(super) fn exponent(bytes: &[u8], mut index: usize, hex: bool) -> Option<(usize, i64)> {
    let marker = if hex { b'p' } else { b'e' };
    if !bytes
        .get(index)
        .is_some_and(|byte| byte.eq_ignore_ascii_case(&marker))
    {
        return (!hex).then_some((index, 0));
    }
    index += 1;
    let negative = bytes.get(index) == Some(&b'-');
    if matches!(bytes.get(index), Some(b'+' | b'-')) {
        index += 1;
    }
    if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut exponent = 0_i64;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'_' {
            index += 1;
            continue;
        }
        if !byte.is_ascii_digit() {
            break;
        }
        // Go caps the accumulator after it reaches 10000.
        if exponent < 10_000 {
            exponent = exponent * 10 + i64::from(byte - b'0');
        }
        index += 1;
    }
    Some((index, if negative { -exponent } else { exponent }))
}

fn digit(byte: u8, hex: bool) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' if hex => Some(byte - b'a' + 10),
        b'A'..=b'F' if hex => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn underscores_valid(bytes: &[u8]) -> bool {
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let hex = bytes
        .get(index..index + 2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"0x"));
    let mut previous_digit = false;
    if hex {
        index += 2;
        previous_digit = true;
    }
    let mut underscore = false;
    for &byte in &bytes[index..] {
        if digit(byte, hex).is_some() {
            previous_digit = true;
            underscore = false;
        } else if byte == b'_' {
            if !previous_digit {
                return false;
            }
            previous_digit = false;
            underscore = true;
        } else {
            if underscore {
                return false;
            }
            previous_digit = false;
        }
    }
    !underscore
}

fn hex_float(number: Number) -> Option<f64> {
    let Number {
        mut mantissa,
        exponent,
        negative,
        truncated,
        ..
    } = number;
    let mut exponent = exponent + 52;
    while mantissa != 0 && mantissa >> 54 == 0 {
        mantissa <<= 1;
        exponent -= 1;
    }
    if truncated {
        mantissa |= 1;
    }
    while mantissa >> 55 != 0 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exponent += 1;
    }
    while mantissa > 1 && exponent < -1024 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exponent += 1;
    }
    let mut round = mantissa & 3;
    mantissa >>= 2;
    round |= mantissa & 1;
    exponent += 2;
    if round == 3 {
        mantissa += 1;
        if mantissa == 1 << 53 {
            mantissa >>= 1;
            exponent += 1;
        }
    }
    if mantissa >> 52 == 0 {
        exponent = -1023;
    }
    if exponent > 1023 {
        return None;
    }
    let mut bits = mantissa & ((1_u64 << 52) - 1);
    // Conversion is bounded above and below by normalization/denormalization.
    bits |= u64::try_from(exponent + 1023).ok()? << 52;
    if negative {
        bits |= 1 << 63;
    }
    Some(f64::from_bits(bits))
}
