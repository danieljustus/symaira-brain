//! Frozen Go Retry-After parsing and SDK target-specific integer formatting.

/// Parses Go's delta-seconds input, including its special and hexadecimal
/// float syntax. A malformed, overflowing numeric, or negative input has no
/// usable retry delay. Negative zero and unsigned NaN follow Go's comparison.
#[must_use]
pub fn parse_retry_after(value: &str) -> Option<f64> {
    let seconds = super::go_float::parse(value.trim())?;
    (seconds >= 0.0 || seconds.is_nan()).then_some(seconds)
}

pub(super) fn seconds(value: &str) -> Option<isize> {
    parse_retry_after(value).map(go_int)
}

#[allow(clippy::cast_possible_truncation)]
fn go_int(value: f64) -> isize {
    // The pinned SDK's default AMD64 rules lower float64->int to CVTTSD2SQ
    // (386 uses CVTTSD2SL): invalid conversion returns the signed minimum.
    // ARM64 lowers to FCVTZSD, matching Rust's NaN0/saturating conversion.
    // The native SDK oracle must validate each release target independently.
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        let boundary = if isize::BITS == 64 {
            9_223_372_036_854_775_808.0
        } else {
            2_147_483_648.0
        };
        if !value.is_finite() || value >= boundary {
            return isize::MIN;
        }
    }
    value as isize
}
