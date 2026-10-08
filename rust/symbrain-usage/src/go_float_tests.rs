//! Go `strconv.ParseFloat` special, hexadecimal and underscore grammar.
use super::go_float::parse;
use super::parse_retry_after;

fn bits(value: &str) -> Option<u64> {
    parse(value).map(f64::to_bits)
}

#[test]
fn specials_match_go_spellings_and_sign() {
    // Go returns its own NaN payload for every NaN spelling.
    assert_eq!(bits("nan"), Some(0x7ff8_0000_0000_0001));
    assert_eq!(bits("NaN"), Some(0x7ff8_0000_0000_0001));
    for (text, expected) in [
        ("inf", f64::INFINITY),
        ("+Inf", f64::INFINITY),
        ("Infinity", f64::INFINITY),
        ("-inf", f64::NEG_INFINITY),
        ("-INFINITY", f64::NEG_INFINITY),
    ] {
        assert_eq!(parse(text), Some(expected), "{text}");
    }
    // Signed NaN and partial words are not Go special values.
    assert_eq!(parse("-nan"), None);
    assert_eq!(parse("infin"), None);
}

#[test]
fn hexadecimal_floats_round_like_go() {
    for (text, expected) in [
        ("0x1p-2", 0.25),
        ("0X1.8P1", 3.0),
        ("0x.8p1", 1.0),
        ("-0x1p0", -1.0),
        ("0x0p0", 0.0),
        ("0x1_0p0", 16.0),
        ("0x_1p0", 1.0),
        // Ties round to even; bits beyond 16 kept digits stay sticky.
        ("0x1.fffffffffffff8p0", 2.0),
        ("0x1.00000000000000001p0", 1.0),
        ("0x1p+1_0", 1024.0),
    ] {
        assert_eq!(parse(text), Some(expected), "{text}");
    }
    // Smallest subnormal, and its exact half rounding to even zero.
    assert_eq!(bits("0x1p-1074"), Some(1));
    assert_eq!(bits("0x1p-1075"), Some(0));
    assert_eq!(bits("0x1.8p-1074"), Some(2));
    assert_eq!(bits("-0x1p-1074"), Some(0x8000_0000_0000_0001));
    // Largest finite value; one more binade overflows (Go reports ErrRange).
    assert_eq!(parse("0x1.fffffffffffffp1023"), Some(f64::MAX));
    assert_eq!(parse("0x1p1024"), None);
}

#[test]
fn malformed_numbers_are_rejected() {
    for text in [
        "", "+", "-", ".", "0x", "0xp1", "0x1", "0x1p", "0x1p+", "1e", "1e+", "1_", "_1", "1__0",
        "1_.5", "0x1__0p0", "1.5x", "0x1.8q1", "1.2.3",
    ] {
        assert_eq!(parse(text), None, "{text:?}");
    }
}

#[test]
fn retry_after_admits_only_non_negative_or_nan_delays() {
    assert_eq!(parse_retry_after(" 0x1p2 "), Some(4.0));
    assert_eq!(parse_retry_after("Infinity"), Some(f64::INFINITY));
    assert!(parse_retry_after("NaN").is_some_and(f64::is_nan));
    assert_eq!(
        parse_retry_after("-0").map(f64::to_bits),
        Some((-0.0_f64).to_bits())
    );
    assert_eq!(parse_retry_after("-0x1p0"), None);
    assert_eq!(parse_retry_after("-inf"), None);
    assert_eq!(parse_retry_after("soon"), None);
}
