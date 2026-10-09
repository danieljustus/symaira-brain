//! Finite JSON numbers with the pinned Go 1.26.7 binary64 conversion.
//! Private conversion is a byte-identical copy of the reviewed Retry parser;
//! header admission, trimming, special values and integer formatting stay there.
mod go_decimal;
mod go_decimal_fast;
mod go_decimal_shift;
mod go_float;

#[cfg(test)]
#[path = "go_json_float_tests.rs"]
mod sdk_tests;

/// Converts one complete JSON number using Go's bounded mantissa, Eisel-Lemire
/// and 800-digit fallback paths. Signed zero and finite underflow are retained.
/// Returns `None` for non-JSON grammar or Go's range error (including overflow).
/// No whitespace, hexadecimal, underscores, `NaN` or infinity is admitted.
#[must_use]
pub fn parse_finite(value: &str) -> Option<f64> {
    json_number(value.as_bytes()).then_some(())?;
    go_float::parse(value).filter(|number| number.is_finite())
}

fn json_number(bytes: &[u8]) -> bool {
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if start == index {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if start == index {
            return false;
        }
    }
    index == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::parse_finite;

    #[test]
    fn finite_json_domain_is_distinct_from_retry_header_admission() {
        for invalid in [
            "",
            "-",
            "+1",
            "01",
            "-01",
            ".1",
            "1.",
            "1e",
            "1e+",
            "1_0",
            "0x1p2",
            "NaN",
            "Inf",
            "-Infinity",
            " 1",
            "1\n",
            "1e9999",
            "-1e9999",
        ] {
            assert_eq!(parse_finite(invalid), None, "{invalid}");
        }
        for (input, expected) in [
            ("0", 0.0_f64),
            ("-0", -0.0),
            ("-0.0e99", -0.0),
            ("-3", -3.0),
            ("0.1", 0.1),
            ("1e-9999", 0.0),
            ("-1e-9999", -0.0),
        ] {
            assert_eq!(
                parse_finite(input).unwrap().to_bits(),
                expected.to_bits(),
                "{input}"
            );
        }
    }

    #[test]
    fn long_decimal_retains_historical_actual_go_fallback_bits() {
        // Root's actual pinned Go SDK result, not a current native observation.
        let input = format!("0.{}1e100000", "0".repeat(10_000));
        assert_eq!(
            parse_finite(&input).unwrap().to_bits(),
            0x3fb9_9999_9999_999a
        );
        assert_eq!(
            parse_finite(&format!("-{input}")).unwrap().to_bits(),
            0xbfb9_9999_9999_999a
        );
    }
}
