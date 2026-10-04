//! Go1.26.7 decimal fast/fallback control flow and bounded decimal state.
//! Adapted from internal/strconv atof.go and decimal.go; Go Authors BSD notice
//! is retained in migration/licenses/go-strconv-bsd.txt. No input-size waiver.
use super::go_decimal_fast::{eisel, exact};
use super::go_float::Number;

pub(super) fn parse(value: &str, number: &Number) -> Option<f64> {
    let Number {
        mantissa,
        exponent,
        negative,
        truncated,
        ..
    } = *number;
    if !truncated && let Some(value) = exact(mantissa, exponent, negative) {
        return Some(value);
    }
    if let Some(value) = eisel(mantissa, exponent, negative)
        && (!truncated || eisel(mantissa + 1, exponent, negative) == Some(value))
    {
        return Some(value);
    }
    // The fallback deliberately rereads the original valid text. Its 800-digit
    // cap changes decimal-point accounting relative to readFloat; normalizing
    // that earlier scan would change accepted long-input results.
    Decimal::set(value)?.float()
}

pub(super) struct Decimal {
    pub(super) digits: [u8; 800],
    pub(super) length: usize,
    pub(super) point: i64,
    negative: bool,
    pub(super) truncated: bool,
}

impl Decimal {
    fn set(value: &str) -> Option<Self> {
        let mut result = Self {
            digits: [0; 800],
            length: 0,
            point: 0,
            negative: value.starts_with('-'),
            truncated: false,
        };
        let bytes = value.as_bytes();
        let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
        let mut dot = false;
        while let Some(&byte) = bytes.get(index) {
            match byte {
                b'_' => {}
                b'.' => {
                    dot = true;
                    result.point = i64::try_from(result.length).ok()?;
                }
                b'0' if result.length == 0 => {
                    result.point -= 1;
                }
                b'0'..=b'9' => {
                    if result.length < result.digits.len() {
                        result.digits[result.length] = byte;
                        result.length += 1;
                    } else if byte != b'0' {
                        result.truncated = true;
                    }
                }
                _ => break,
            }
            index += 1;
        }
        if !dot {
            result.point = i64::try_from(result.length).ok()?;
        }
        let (end, exponent) = super::go_float::exponent(bytes, index, false)?;
        if end != bytes.len() {
            return None;
        }
        result.point += exponent;
        Some(result)
    }

    pub(super) fn trim(&mut self) {
        while self.length > 0 && self.digits[self.length - 1] == b'0' {
            self.length -= 1;
        }
        if self.length == 0 {
            self.point = 0;
        }
    }

    fn float(mut self) -> Option<f64> {
        const POWERS: [i64; 9] = [1, 3, 6, 9, 13, 16, 19, 23, 26];
        let sign = if self.negative { 1_u64 << 63 } else { 0 };
        if self.length == 0 || self.point < -330 {
            return Some(f64::from_bits(sign));
        }
        if self.point > 310 {
            return None;
        }
        let mut exponent = 0_i64;
        while self.point > 0 {
            let shift = usize::try_from(self.point)
                .ok()
                .and_then(|p| POWERS.get(p))
                .copied()
                .unwrap_or(27);
            self.shift(-shift);
            exponent += shift;
        }
        while self.point < 0 || self.point == 0 && self.digits[0] < b'5' {
            let shift = usize::try_from(-self.point)
                .ok()
                .and_then(|p| POWERS.get(p))
                .copied()
                .unwrap_or(27);
            self.shift(shift);
            exponent -= shift;
        }
        exponent -= 1;
        if exponent < -1022 {
            self.shift(exponent + 1022);
            exponent = -1022;
        }
        self.finish(exponent, sign)
    }

    fn finish(mut self, mut exponent: i64, sign: u64) -> Option<f64> {
        if exponent >= 1024 {
            return None;
        }
        self.shift(53);
        let mut mantissa = self.rounded_integer();
        if mantissa == 1 << 53 {
            mantissa >>= 1;
            exponent += 1;
            if exponent >= 1024 {
                return None;
            }
        }
        if mantissa & (1 << 52) == 0 {
            exponent = -1023;
        }
        let bits =
            sign | (u64::try_from(exponent + 1023).ok()? << 52) | (mantissa & ((1_u64 << 52) - 1));
        Some(f64::from_bits(bits))
    }

    fn rounded_integer(&self) -> u64 {
        // Go shouldRoundUp rejects a negative digit index. Converting it to
        // zero would spuriously round tiny subnormals using their first digit.
        if self.point < 0 {
            return 0;
        }
        if self.point > 20 {
            return u64::MAX;
        }
        let mut value = 0_u64;
        let end = usize::try_from(self.point).unwrap_or(0);
        for index in 0..end {
            value = value * 10
                + self
                    .digits
                    .get(index)
                    .filter(|_| index < self.length)
                    .map_or(0, |digit| u64::from(digit - b'0'));
        }
        let round = end < self.length
            && (self.digits[end] > b'5'
                || self.digits[end] == b'5'
                    && (end + 1 != self.length
                        || self.truncated
                        || end > 0 && (self.digits[end - 1] - b'0') % 2 != 0));
        value + u64::from(round)
    }
}
