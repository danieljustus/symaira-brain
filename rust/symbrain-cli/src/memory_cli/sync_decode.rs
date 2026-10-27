//! Decoding of the relay's `/api/sync/changes` reply, limited to the shapes
//! whose shipped (`encoding/json`) behaviour is reproduced exactly.
//!
//! A reply that carries data to apply (memories, tombstones, a pagination
//! cursor), a time spelling other than `...Z`, or JSON the shipped decoder
//! words differently is [`Decoded::Handoff`]: the whole invocation stays on
//! the shipped implementation.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::error::Category;
use symbrain_memory::zero_time;

pub(super) enum Decoded<T> {
    Value(T),
    /// The text after `decode remote response: `.
    Failure(String),
    Handoff,
}

/// One JSON value reduced to what the sync reply needs.
enum Slot {
    Null,
    Text(String),
    EmptyList,
    Other,
}

struct SlotVisitor;

impl<'de> Visitor<'de> for SlotVisitor {
    type Value = Slot;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("any JSON value")
    }
    fn visit_unit<E>(self) -> Result<Slot, E> {
        Ok(Slot::Null)
    }
    fn visit_none<E>(self) -> Result<Slot, E> {
        Ok(Slot::Null)
    }
    fn visit_str<E>(self, value: &str) -> Result<Slot, E> {
        Ok(Slot::Text(value.to_owned()))
    }
    fn visit_bool<E>(self, _: bool) -> Result<Slot, E> {
        Ok(Slot::Other)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Slot, E> {
        Ok(Slot::Other)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Slot, E> {
        Ok(Slot::Other)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Slot, E> {
        Ok(Slot::Other)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Slot, A::Error> {
        let mut empty = true;
        while seq.next_element::<IgnoredAny>()?.is_some() {
            empty = false;
        }
        Ok(if empty { Slot::EmptyList } else { Slot::Other })
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Slot, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Slot::Other)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SlotVisitor)
    }
}

/// A JSON object as ordered key/value pairs, duplicates included.
struct Pairs(Vec<(String, Slot)>);

impl<'de> Deserialize<'de> for Pairs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PairsVisitor;
        impl<'de> Visitor<'de> for PairsVisitor {
            type Value = Pairs;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Pairs, A::Error> {
                let mut pairs = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    pairs.push((key, map.next_value::<Slot>()?));
                }
                Ok(Pairs(pairs))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Pairs, E> {
                Ok(Pairs(Vec::new()))
            }
        }
        deserializer.deserialize_any(PairsVisitor)
    }
}

/// Go's case folding for the ASCII field names used here: the long s and the
/// Kelvin sign fold onto `s` and `k`.
fn fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{17f}' => 's',
            '\u{212a}' => 'k',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// `encoding/json` refuses nesting beyond 10000 levels while `serde_json` has
/// its own, different limits; deeply nested bodies are never claimed.
const NESTING_CLAIM: usize = 1000;

fn nests_deeply(body: &[u8]) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for &byte in body {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > NESTING_CLAIM {
                        return true;
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

fn first_value(body: &[u8]) -> Result<Pairs, serde_json::Error> {
    serde_json::Deserializer::from_slice(body)
        .into_iter::<Pairs>()
        .next()
        .unwrap_or_else(|| Ok(Pairs(Vec::new())))
}

/// `quoteChar` of `encoding/json`: the byte is quoted as the rune of the same
/// value with `strconv.Quote` rules.
fn quote_char(byte: u8) -> String {
    match byte {
        b'\'' => "'\\''".to_owned(),
        b'\\' => "'\\\\'".to_owned(),
        0x07 => "'\\a'".to_owned(),
        0x08 => "'\\b'".to_owned(),
        0x0b => "'\\v'".to_owned(),
        0x0c => "'\\f'".to_owned(),
        0x00..=0x1f | 0x7f => format!("'\\x{byte:02x}'"),
        0x80..=0xa0 | 0xad => format!("'\\u{byte:04x}'"),
        other => format!("'{}'", char::from(other)),
    }
}

/// Decodes a success reply into the relay's clock (`ChangesResponse.ServerTime`).
pub(super) fn changes(body: &[u8]) -> Decoded<DateTime<Utc>> {
    let start = body
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'));
    let Some(start) = start else {
        return Decoded::Failure("EOF".to_owned());
    };
    let trimmed = &body[start..];
    match trimmed[0] {
        b'n' if trimmed.trim_ascii_end() == b"null" => return Decoded::Value(zero_time()),
        b'{' => {}
        b'[' | b'"' | b'-' | b'0'..=b'9' | b'n' | b't' | b'f' => return Decoded::Handoff,
        byte => {
            return Decoded::Failure(format!(
                "invalid character {} looking for beginning of value",
                quote_char(byte)
            ));
        }
    }
    if nests_deeply(trimmed) {
        return Decoded::Handoff;
    }
    let pairs = match first_value(trimmed) {
        Ok(pairs) => pairs.0,
        Err(error) if error.classify() == Category::Eof => {
            return Decoded::Failure("unexpected EOF".to_owned());
        }
        Err(_) => return Decoded::Handoff,
    };
    let mut server_time = zero_time();
    for (key, slot) in pairs {
        let folded = fold(&key);
        let known =
            ["memories", "deleted", "server_time", "next_cursor"].contains(&folded.as_str());
        if known && folded != key {
            return Decoded::Handoff;
        }
        match (key.as_str(), slot) {
            ("memories" | "deleted", Slot::Null | Slot::EmptyList)
            | ("next_cursor" | "server_time", Slot::Null) => {}
            ("next_cursor", Slot::Text(text)) if text.is_empty() => {}
            ("server_time", Slot::Text(text)) => match parse_server_time(&text) {
                Some(time) => server_time = time,
                None => return Decoded::Handoff,
            },
            ("memories" | "deleted" | "next_cursor" | "server_time", _) => {
                return Decoded::Handoff;
            }
            _ => {}
        }
    }
    Decoded::Value(server_time)
}

/// `time.Time.UnmarshalJSON` for the canonical UTC spelling
/// `YYYY-MM-DDTHH:MM:SS[.f{1,9}]Z`; any other spelling is not claimed.
pub(super) fn parse_server_time(text: &str) -> Option<DateTime<Utc>> {
    let body = text.strip_suffix('Z')?;
    let (stamp, fraction) = match body.split_once('.') {
        Some((stamp, fraction)) => (stamp, Some(fraction)),
        None => (body, None),
    };
    let bytes = stamp.as_bytes();
    if bytes.len() != 19 || !stamp.is_ascii() {
        return None;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let marks = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if !digits.iter().all(|&at| bytes[at].is_ascii_digit())
        || !marks.iter().all(|&(at, byte)| bytes[at] == byte)
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| stamp[range].parse::<u32>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if year == 0 {
        return None;
    }
    let nanos = match fraction {
        None => 0,
        Some(digits) => {
            if digits.is_empty()
                || digits.len() > 9
                || !digits.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            format!("{digits:0<9}").parse::<u32>().ok()?
        }
    };
    let date = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)?;
    let naive: NaiveDateTime = date.and_hms_nano_opt(hour, minute, second, 0)?;
    Some(Utc.from_utc_datetime(&naive.with_nanosecond(nanos)?))
}

/// The `error`/`code` pair of a non-success reply (`HTTPError`); `None` hands
/// the invocation off.
pub(super) fn api_error(body: &[u8]) -> Option<(String, String)> {
    if std::str::from_utf8(body).is_err() {
        return None;
    }
    let start = body
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'));
    let trimmed = &body[start.unwrap_or(body.len())..];
    if nests_deeply(trimmed) {
        return None;
    }
    let pairs = match first_value(trimmed) {
        Ok(pairs) => pairs.0,
        // Shapes the decoder refuses leave both fields empty.
        Err(error) => {
            let text = error.to_string();
            if text.contains("recursion limit") || text.contains("out of range") {
                return None;
            }
            return Some((String::new(), String::new()));
        }
    };
    let (mut message, mut code) = (String::new(), String::new());
    for (key, slot) in pairs {
        if let Slot::Text(text) = slot {
            match fold(&key).as_str() {
                "error" => message = text,
                "code" => code = text,
                _ => {}
            }
        }
    }
    Some((message, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(text: &str) -> DateTime<Utc> {
        parse_server_time(text).unwrap()
    }

    #[test]
    fn empty_replies_decode_to_the_relay_clock() {
        for (body, expected) in [
            (&b"{}"[..], zero_time()),
            (b" null ", zero_time()),
            (
                br#"{"memories":null,"deleted":[],"next_cursor":"","server_time":"2026-10-26T01:02:03.5Z"}"#,
                time("2026-10-26T01:02:03.5Z"),
            ),
            (br#"{"other":[1,{"a":1e999}]}"#, zero_time()),
            (
                br#"{"server_time":"2026-10-26T01:02:03Z","server_time":null}"#,
                time("2026-10-26T01:02:03Z"),
            ),
            (br"{} trailing", zero_time()),
        ] {
            let Decoded::Value(actual) = changes(body) else {
                panic!("not decoded: {}", String::from_utf8_lossy(body));
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn go_wording_for_plain_failures() {
        for (body, expected) in [
            (&b""[..], "EOF"),
            (b"  \n", "EOF"),
            (b"x", "invalid character 'x' looking for beginning of value"),
            (b"}", "invalid character '}' looking for beginning of value"),
            (
                b"'",
                "invalid character '\\'' looking for beginning of value",
            ),
            (
                b"\\",
                "invalid character '\\\\' looking for beginning of value",
            ),
            (
                b"\xef",
                "invalid character 'ï' looking for beginning of value",
            ),
            (
                b"\xff",
                "invalid character 'ÿ' looking for beginning of value",
            ),
            (
                b"\x01",
                "invalid character '\\x01' looking for beginning of value",
            ),
            (
                b"\x0c",
                "invalid character '\\f' looking for beginning of value",
            ),
            (
                b"\x7f",
                "invalid character '\\x7f' looking for beginning of value",
            ),
            (
                b"\x85",
                "invalid character '\\u0085' looking for beginning of value",
            ),
            (
                b"\xa0",
                "invalid character '\\u00a0' looking for beginning of value",
            ),
            (
                b"\xad",
                "invalid character '\\u00ad' looking for beginning of value",
            ),
            (br#"{"server_time""#, "unexpected EOF"),
        ] {
            let Decoded::Failure(text) = changes(body) else {
                panic!("not a failure: {}", String::from_utf8_lossy(body));
            };
            assert_eq!(text, expected);
        }
    }

    #[test]
    fn everything_else_is_handed_off() {
        for body in [
            &br#"{"memories":[{"id":"a"}]}"#[..],
            br#"{"deleted":[{"id":"a"}]}"#,
            br#"{"next_cursor":"abc"}"#,
            br#"{"server_time":"2026-10-26T03:02:03+02:00"}"#,
            br#"{"server_time":"bad"}"#,
            br#"{"server_time":5}"#,
            br#"{"Server_Time":"2026-10-26T01:02:03Z"}"#,
            br#"{"memories":{}}"#,
            &[&b"{\"x\":"[..], &[b'['; 1001], &[b']'; 1001], b"}"].concat(),
            b"[]",
            b"nul",
            b"1",
            b"{\"a\":}",
        ] {
            assert!(
                matches!(changes(body), Decoded::Handoff),
                "claimed: {}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn server_time_accepts_only_the_canonical_utc_spelling() {
        assert!(parse_server_time("0001-01-01T00:00:00Z").is_some());
        assert_eq!(time("0001-01-01T00:00:00Z"), zero_time());
        for bad in [
            "2026-10-26T01:02:03",
            "2026-10-26t01:02:03Z",
            "2026-10-26 01:02:03Z",
            "2026-10-26T01:02:03z",
            "2026-02-30T01:02:03Z",
            "2026-10-26T24:02:03Z",
            "2026-10-26T01:02:60Z",
            "0000-10-26T01:02:03Z",
            "2026-10-26T01:02:03.Z",
            "2026-10-26T01:02:03.1234567890Z",
            "2026-10-26T01:02:03+00:00",
        ] {
            assert!(parse_server_time(bad).is_none(), "accepted: {bad}");
        }
    }

    #[test]
    fn api_errors_follow_the_shipped_decoder() {
        let pair = |body: &[u8]| api_error(body).unwrap();
        assert_eq!(
            pair(br#"{"error":"bad","code":"E1"}"#),
            ("bad".into(), "E1".into())
        );
        assert_eq!(
            pair(br#"{"ERROR":"up","Code":"x"}"#),
            ("up".into(), "x".into())
        );
        assert_eq!(
            pair(br#"{"error":5,"code":"c"}"#),
            (String::new(), "c".into())
        );
        assert_eq!(
            pair(br#"{"error":"a","error":"b"}"#),
            ("b".into(), String::new())
        );
        for empty in [&b"<html>"[..], b"", b"[1]", b"null", b"{\"error\":\"par"] {
            assert_eq!(pair(empty), (String::new(), String::new()));
        }
        assert!(api_error(&[b'{', 0xff]).is_none());
    }
}
