//! Go `ParseFloat` syntax for string-valued memory configuration fields.

pub(super) fn validate(text: &str) -> Option<()> {
    if text.eq_ignore_ascii_case("nan") {
        return Some(());
    }
    let text = text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('+'))
        .unwrap_or(text);
    if text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("infinity") {
        return Some(());
    }
    let hex = text.starts_with("0x") || text.starts_with("0X");
    let text = if hex { &text[2..] } else { text };
    let split = if hex {
        text.find(['p', 'P'])
    } else {
        text.find(['e', 'E'])
    };
    if hex && split.is_none() {
        return None;
    }
    let (mantissa, exponent) = split.map_or((text, None), |index| {
        (&text[..index], Some(&text[index + 1..]))
    });
    if let Some(exponent) = exponent {
        let exponent = exponent
            .strip_prefix('-')
            .or_else(|| exponent.strip_prefix('+'))
            .unwrap_or(exponent);
        valid_digits(exponent, false, false)?;
    }
    let mut parts = mantissa.split('.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some() || (integer.is_empty() && fraction.is_none_or(str::is_empty)) {
        return None;
    }
    if !integer.is_empty() {
        valid_digits(integer, hex, hex)?;
    }
    if let Some(fraction) = fraction.filter(|part| !part.is_empty()) {
        valid_digits(fraction, hex, false)?;
    }
    if hex {
        validate_hex_range(integer, fraction.unwrap_or(""), exponent?)
    } else {
        text.replace('_', "")
            .parse::<f64>()
            .ok()?
            .is_finite()
            .then_some(())
    }
}

fn validate_hex_range(integer: &str, fraction: &str, exponent: &str) -> Option<()> {
    // Compare exact bits to the round-to-infinity boundary, without an
    // intermediate float overflow for long mantissas and negative exponents.
    let point = i64::try_from(integer.bytes().filter(|byte| *byte != b'_').count()).ok()?;
    let digits: Vec<u8> = integer
        .bytes()
        .chain(fraction.bytes())
        .filter(|byte| *byte != b'_')
        .map(digit)
        .collect::<Option<_>>()?;
    let Some(first) = digits.iter().position(|value| *value != 0) else {
        return Some(());
    };
    let leading_bit = digits[first].ilog2();
    let exponent = exponent.replace('_', "");
    let exponent = exponent.parse::<i64>().unwrap_or_else(|_| {
        if exponent.starts_with('-') {
            i64::MIN
        } else {
            i64::MAX
        }
    });
    let power = exponent
        .saturating_add(4 * (point - i64::try_from(first).ok()? - 1))
        .saturating_add(i64::from(leading_bit));
    if power < 1023 {
        return Some(());
    }
    if power > 1023 {
        return None;
    }
    let bits: Vec<bool> = digits[first..]
        .iter()
        .flat_map(|value| (0..4).rev().map(move |shift| value & (1 << shift) != 0))
        .skip(usize::try_from(3 - leading_bit).ok()?)
        .take(54)
        .collect();
    // 2^1024 - 2^970 rounds to infinity. Anything below remains finite.
    (!(bits.len() == 54 && bits.iter().all(|value| *value))).then_some(())
}

fn valid_digits(text: &str, hex: bool, prefix: bool) -> Option<()> {
    let mut previous_digit = prefix;
    for byte in text.bytes() {
        if byte == b'_' {
            if !previous_digit {
                return None;
            }
            previous_digit = false;
        } else {
            if digit(byte)? >= if hex { 16 } else { 10 } {
                return None;
            }
            previous_digit = true;
        }
    }
    previous_digit.then_some(())
}

fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
