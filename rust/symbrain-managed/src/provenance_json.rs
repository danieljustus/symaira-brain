//! Go encoding/json string replacement and time.Time validation for sidecars.

pub(super) fn replace_invalid_strings(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            in_string = !in_string;
        } else if in_string && bytes[index] == b'\\' {
            if let Some(unit) = unicode_escape(&bytes[index..]) {
                if (0xd800..=0xdbff).contains(&unit)
                    && unicode_escape(bytes.get(index + 6..).unwrap_or_default())
                        .is_some_and(|low| (0xdc00..=0xdfff).contains(&low))
                {
                    result.extend_from_slice(&bytes[index..index + 12]);
                    index += 12;
                    continue;
                }
                if (0xd800..=0xdfff).contains(&unit) {
                    result.extend_from_slice(b"\\ufffd");
                    index += 6;
                    continue;
                }
            }
            result.push(bytes[index]);
            index += 1;
            if index == bytes.len() {
                break;
            }
        }
        result.push(bytes[index]);
        index += 1;
    }
    result
}

fn unicode_escape(bytes: &[u8]) -> Option<u16> {
    if !bytes.starts_with(b"\\u") {
        return None;
    }
    let digits = std::str::from_utf8(bytes.get(2..6)?).ok()?;
    u16::from_str_radix(digits, 16).ok()
}

// Go 1.26's strict RFC3339 routine intentionally retains its Parse fallback:
// one-digit hours, comma fractions, offset hour 24 and offset minute 60 work.
pub(super) fn valid_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return false;
    }
    let year = decimal(bytes.get(..4).unwrap_or_default());
    let month = decimal(bytes.get(5..7).unwrap_or_default());
    let day = decimal(bytes.get(8..10).unwrap_or_default());
    if !year
        .zip(month)
        .zip(day)
        .is_some_and(|((year, month), day)| {
            chrono::NaiveDate::from_ymd_opt(i32::try_from(year).unwrap_or(-1), month, day).is_some()
        })
    {
        return false;
    }
    let time = &bytes[11..];
    let Some(colon) = time.iter().position(|byte| *byte == b':') else {
        return false;
    };
    if !(1..=2).contains(&colon) || decimal(&time[..colon]).is_none_or(|hour| hour >= 24) {
        return false;
    }
    let rest = &time[colon + 1..];
    if rest.len() < 6
        || rest[2] != b':'
        || decimal(&rest[..2]).is_none_or(|minute| minute >= 60)
        || decimal(&rest[3..5]).is_none_or(|second| second >= 60)
    {
        return false;
    }
    let mut suffix = &rest[5..];
    if matches!(suffix.first(), Some(b'.' | b',')) {
        suffix = &suffix[1..];
        let digits = suffix
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 {
            return false;
        }
        suffix = &suffix[digits..];
    }
    suffix == b"Z"
        || (suffix.len() == 6
            && matches!(suffix[0], b'+' | b'-')
            && suffix[3] == b':'
            && decimal(&suffix[1..3]).is_some_and(|hour| hour <= 24)
            && decimal(&suffix[4..6]).is_some_and(|minute| minute <= 60))
}

fn decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}
