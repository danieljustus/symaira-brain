//! Accept/reject port of the `time.ParseInLocation` calls in BurntSushi/toml's
//! `valueDatetime`. Only validity matters: every failure reports the same
//! `invalid datetime: "..."` message.

#[derive(Clone, Copy)]
enum Elem {
    Lit(u8),
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Fraction,
    Zone,
}

use Elem::{Day, Fraction, Hour, Lit, Minute, Month, Second, Year, Zone};

const DATE: [Elem; 5] = [Year, Lit(b'-'), Month, Lit(b'-'), Day];

/// The layouts of Go's `dtTypes`, in order, with their literal layout text
/// (needed by `missingLeadingZero`).
const LAYOUTS: [(&str, &[Elem]); 7] = [
    (
        "2006-01-02T15:04:05.999999999Z07:00",
        &[
            Year,
            Lit(b'-'),
            Month,
            Lit(b'-'),
            Day,
            Lit(b'T'),
            Hour,
            Lit(b':'),
            Minute,
            Lit(b':'),
            Second,
            Fraction,
            Zone,
        ],
    ),
    (
        "2006-01-02T15:04:05.999999999",
        &[
            Year,
            Lit(b'-'),
            Month,
            Lit(b'-'),
            Day,
            Lit(b'T'),
            Hour,
            Lit(b':'),
            Minute,
            Lit(b':'),
            Second,
            Fraction,
        ],
    ),
    ("2006-01-02", &DATE),
    (
        "15:04:05.999999999",
        &[Hour, Lit(b':'), Minute, Lit(b':'), Second, Fraction],
    ),
    (
        "2006-01-02T15:04Z07:00",
        &[
            Year,
            Lit(b'-'),
            Month,
            Lit(b'-'),
            Day,
            Lit(b'T'),
            Hour,
            Lit(b':'),
            Minute,
            Zone,
        ],
    ),
    (
        "2006-01-02T15:04",
        &[
            Year,
            Lit(b'-'),
            Month,
            Lit(b'-'),
            Day,
            Lit(b'T'),
            Hour,
            Lit(b':'),
            Minute,
        ],
    ),
    ("15:04", &[Hour, Lit(b':'), Minute]),
];

/// `datetimeRepl` followed by the `dtTypes` loop and `missingLeadingZero`.
/// Returns the replaced value and whether it is a valid TOML datetime.
pub(super) fn validate(raw: &str) -> (String, bool) {
    let replaced: String = raw
        .chars()
        .map(|ch| match ch {
            'z' => 'Z',
            't' | ' ' => 'T',
            other => other,
        })
        .collect();
    for (layout, elems) in LAYOUTS {
        if parse(elems, replaced.as_bytes()) {
            return if missing_leading_zero(replaced.as_bytes(), layout.as_bytes()) {
                (replaced, false)
            } else {
                (replaced, true)
            };
        }
    }
    (replaced, false)
}

fn missing_leading_zero(value: &[u8], layout: &[u8]) -> bool {
    for (index, &layout_byte) in layout.iter().enumerate() {
        if layout_byte == b'.' || layout_byte == b'Z' {
            return false;
        }
        if !layout_byte.is_ascii_digit() && value.get(index) != Some(&layout_byte) {
            return true;
        }
    }
    false
}

fn digit(value: &[u8], index: usize) -> bool {
    value.get(index).is_some_and(u8::is_ascii_digit)
}

/// Go's `getnum`.
fn getnum(value: &[u8], fixed: bool) -> Option<(u32, &[u8])> {
    if !digit(value, 0) {
        return None;
    }
    if !digit(value, 1) {
        if fixed {
            return None;
        }
        return Some((u32::from(value[0] - b'0'), &value[1..]));
    }
    Some((
        u32::from(value[0] - b'0') * 10 + u32::from(value[1] - b'0'),
        &value[2..],
    ))
}

fn days_in(month: u32, year: u32) -> u32 {
    match month {
        2 => {
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn parse(elems: &[Elem], mut value: &[u8]) -> bool {
    let (mut year, mut month, mut day) = (0_u32, 1_u32, 1_u32);
    for elem in elems {
        match *elem {
            Lit(expected) => {
                if value.first() != Some(&expected) {
                    return false;
                }
                value = &value[1..];
            }
            Year => {
                if value.len() < 4 || !value[..4].iter().all(u8::is_ascii_digit) {
                    return false;
                }
                year = value[..4]
                    .iter()
                    .fold(0, |acc, byte| acc * 10 + u32::from(byte - b'0'));
                value = &value[4..];
            }
            Month => {
                let Some((number, rest)) = getnum(value, true) else {
                    return false;
                };
                if !(1..=12).contains(&number) {
                    return false;
                }
                month = number;
                value = rest;
            }
            Day => {
                let Some((number, rest)) = getnum(value, true) else {
                    return false;
                };
                day = number;
                value = rest;
            }
            Hour => {
                let Some((number, rest)) = getnum(value, false) else {
                    return false;
                };
                if number >= 24 {
                    return false;
                }
                value = rest;
            }
            Minute | Second => {
                let Some((number, rest)) = getnum(value, true) else {
                    return false;
                };
                if number >= 60 {
                    return false;
                }
                value = rest;
            }
            Fraction => {
                if matches!(value.first(), Some(b'.' | b',')) && digit(value, 1) {
                    let mut end = 2;
                    while digit(value, end) {
                        end += 1;
                    }
                    value = &value[end..];
                }
            }
            Zone => {
                if value.first() == Some(&b'Z') {
                    value = &value[1..];
                    continue;
                }
                if value.len() < 6 || value[3] != b':' {
                    return false;
                }
                let (Some((hours, _)), Some((minutes, _))) =
                    (getnum(&value[1..3], true), getnum(&value[4..6], true))
                else {
                    return false;
                };
                if hours > 24 || minutes > 60 || !matches!(value[0], b'+' | b'-') {
                    return false;
                }
                value = &value[6..];
            }
        }
    }
    if !value.is_empty() {
        return false;
    }
    // Only date layouts set a day: the time-only layouts keep Go's default.
    day >= 1 && day <= days_in(month, year)
}
