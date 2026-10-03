//! Error-preserving RFC3339 fallback used by Go time.Time JSON records.
use symbrain_core::config::format_go_quoted_bytes;

const LAYOUT: &str = "2006-01-02T15:04:05Z07:00";

pub(super) fn validate(token: &[u8]) -> Result<(), String> {
    if token == b"null" {
        return Ok(());
    }
    if !token.starts_with(b"\"") {
        return Err("Time.UnmarshalJSON: input is not a JSON string".into());
    }
    let value = &token[1..token.len() - 1];
    if std::str::from_utf8(value).is_ok_and(super::json::valid_time) {
        return Ok(());
    }
    let mut remaining = value;
    let mut year = 0;
    let mut month = 0;
    let mut day = 0;
    for (element, min, max, range) in [
        ("2006", 4, 4, 9999),
        ("-", 0, 0, 0),
        ("01", 2, 2, 12),
        ("-", 0, 0, 0),
        ("02", 2, 2, 31),
        ("T", 0, 0, 0),
        ("15", 1, 2, 23),
        (":", 0, 0, 0),
        ("04", 2, 2, 59),
        (":", 0, 0, 0),
        ("05", 2, 2, 59),
    ] {
        let original = remaining;
        if min == 0 {
            remaining = remaining
                .strip_prefix(element.as_bytes())
                .ok_or_else(|| cannot_parse(value, original, element))?;
            continue;
        }
        let digits = remaining
            .iter()
            .copied()
            .take(max)
            .take_while(u8::is_ascii_digit)
            .count();
        if digits < min {
            return Err(cannot_parse(value, original, element));
        }
        let number = std::str::from_utf8(&remaining[..digits])
            .expect("ASCII digits")
            .parse::<u32>()
            .expect("small decimal");
        remaining = &remaining[digits..];
        // Go checks the day against the completed date after time/zone
        // parsing; another malformed field can therefore fail first.
        if element != "02" && (number > range || (element == "01" && number == 0)) {
            let name = match element {
                "01" => "month",
                "15" => "hour",
                "04" => "minute",
                "05" => "second",
                _ => "day",
            };
            return Err(out_of_range(value, name));
        }
        match element {
            "2006" => year = number,
            "01" => month = number,
            "02" => day = number,
            _ => {}
        }
    }
    parse_zone(value, remaining)?;
    if chrono::NaiveDate::from_ymd_opt(i32::try_from(year).expect("year"), month, day).is_none() {
        return Err(out_of_range(value, "day"));
    }
    Ok(())
}

fn quote(value: &[u8]) -> String {
    format_go_quoted_bytes(value)
}
fn cannot_parse(value: &[u8], remaining: &[u8], element: &str) -> String {
    format!(
        "parsing time {} as {}: cannot parse {} as {}",
        quote(value),
        quote(LAYOUT.as_bytes()),
        quote(remaining),
        quote(element.as_bytes())
    )
}
fn out_of_range(value: &[u8], field: &str) -> String {
    format!("parsing time {}: {field} out of range", quote(value))
}

fn parse_zone(value: &[u8], mut remaining: &[u8]) -> Result<(), String> {
    if matches!(remaining.first(), Some(b'.' | b','))
        && remaining.get(1).is_some_and(u8::is_ascii_digit)
    {
        let digits = remaining[1..]
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .count();
        remaining = &remaining[digits + 1..];
    }
    if let Some(suffix) = remaining.strip_prefix(b"Z") {
        remaining = suffix;
    } else {
        let original = remaining;
        let bytes = remaining;
        if bytes.len() < 6
            || !matches!(bytes[0], b'+' | b'-')
            || bytes[3] != b':'
            || !bytes[1..3].iter().all(u8::is_ascii_digit)
            || !bytes[4..6].iter().all(u8::is_ascii_digit)
        {
            return Err(cannot_parse(value, original, "Z07:00"));
        }
        if std::str::from_utf8(&remaining[1..3])
            .expect("ASCII digits")
            .parse::<u32>()
            .expect("hour")
            > 24
        {
            return Err(out_of_range(value, "time zone offset hour"));
        }
        if std::str::from_utf8(&remaining[4..6])
            .expect("ASCII digits")
            .parse::<u32>()
            .expect("minute")
            > 60
        {
            return Err(out_of_range(value, "time zone offset minute"));
        }
        remaining = &remaining[6..];
    }
    if !remaining.is_empty() {
        return Err(format!(
            "parsing time {}: extra text: {}",
            quote(value),
            quote(remaining)
        ));
    }
    Ok(())
}
