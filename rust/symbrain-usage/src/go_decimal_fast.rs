//! Go1.26.7 exact and Eisel-Lemire binary64 paths, including fallback decisions.
//! Adapted from internal/strconv atof.go, atofeisel.go, math.go, pow10tab.go.
//! Copyright 2009-2026 Go Authors; migration/licenses/go-strconv-bsd.txt.
mod powers_negative;
mod powers_positive;

#[allow(clippy::cast_precision_loss)] // SDK bounds this exact conversion to 52 bits.
pub(super) fn exact(mantissa: u64, mut exponent: i64, negative: bool) -> Option<f64> {
    const POWERS: [f64; 23] = [
        1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
        1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
    ];
    if mantissa >> 52 != 0 {
        return None;
    }
    let mut value = mantissa as f64;
    if negative {
        value = -value;
    }
    match exponent {
        0 => Some(value),
        1..=37 => {
            if exponent > 22 {
                value *= POWERS[usize::try_from(exponent - 22).ok()?];
                exponent = 22;
            }
            if !(-1e15..=1e15).contains(&value) {
                return None;
            }
            Some(value * POWERS[usize::try_from(exponent).ok()?])
        }
        -22..=-1 => Some(value / POWERS[usize::try_from(-exponent).ok()?]),
        _ => None,
    }
}

#[allow(clippy::cast_possible_truncation)] // Explicit low/high halves of a full product.
fn multiply(left: u64, right: u64) -> (u64, u64) {
    let product = u128::from(left) * u128::from(right);
    ((product >> 64) as u64, product as u64)
}

pub(super) fn eisel(mut mantissa: u64, exponent: i64, negative: bool) -> Option<f64> {
    let sign = if negative { 1_u64 << 63 } else { 0 };
    if mantissa == 0 {
        return Some(f64::from_bits(sign));
    }
    if !(-348..=347).contains(&exponent) {
        return None;
    }
    let (high, low) = if exponent < 0 {
        powers_negative::POWERS[usize::try_from(exponent + 348).ok()?]
    } else {
        powers_positive::POWERS[usize::try_from(exponent).ok()?]
    };
    let zeros = mantissa.leading_zeros();
    mantissa <<= zeros;
    let mut binary_exponent = 1 + ((exponent * 108_853) >> 15) + 63 + 1023 - i64::from(zeros);
    let (mut hi, mut lo) = multiply(mantissa, high);
    if hi & 0x1ff == 0x1ff && lo.wrapping_add(mantissa) < mantissa {
        let (extra_hi, extra_lo) = multiply(mantissa, low);
        let merged_lo = lo.wrapping_add(extra_hi);
        let merged_hi = hi.wrapping_add(u64::from(merged_lo < lo));
        if merged_hi & 0x1ff == 0x1ff
            && merged_lo.wrapping_add(1) == 0
            && extra_lo.wrapping_add(mantissa) < mantissa
        {
            return None;
        }
        hi = merged_hi;
        lo = merged_lo;
    }
    let leading = hi >> 63;
    let mut result = hi >> (leading + 9);
    binary_exponent -= i64::try_from(1 ^ leading).ok()?;
    if lo == 0 && hi & 0x1ff == 0 && result & 3 == 1 {
        return None;
    }
    result += result & 1;
    result >>= 1;
    if result >> 53 > 0 {
        result >>= 1;
        binary_exponent += 1;
    }
    // Go's wrapping uint test rejects both non-normal and non-finite results.
    if !(1..2047).contains(&binary_exponent) {
        return None;
    }
    Some(f64::from_bits(
        sign | (u64::try_from(binary_exponent).ok()? << 52) | (result & ((1_u64 << 52) - 1)),
    ))
}
