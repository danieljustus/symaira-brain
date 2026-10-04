//! Prepared local parser regressions; actual constructor pairs remain required.
use super::markdown;
use chrono::Timelike;

#[test]
fn six_hour_leap_resource_remains_consolidated_markdown() {
    assert!(markdown::resource(b"2026-01-01T23-59-60-owned-6h-context.md").is_none());
}

#[test]
fn ten_minute_leap_resource_remains_consolidated_markdown() {
    assert!(markdown::resource(b"2026-01-01T23-59-60-owned-10min-context.md").is_none());
}

#[test]
fn ordinary_second_59_retains_both_resource_kinds() {
    for (name, expected) in [
        (b"2026-01-01T23-59-59-owned-6h-context.md".as_slice(), "6h"),
        (
            b"2026-01-01T23-59-59-owned-10min-context.md".as_slice(),
            "10min",
        ),
    ] {
        let (kind, start) = markdown::resource(name).unwrap();
        assert_eq!(kind, expected);
        assert_eq!(start.second(), 59);
        assert_eq!(start.nanosecond(), 0);
    }
}
