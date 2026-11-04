//! Go-exact decoding of one operation-log line.
//!
//! The Go reader (`events.Logger.Read`) calls `json.Unmarshal` per line into
//! an `Event` and skips the line on any error. This module reproduces that
//! decision for every possible input, so the reader never needs the Go
//! implementation:
//!
//! * `json.Unmarshal` first validates the whole line with its scanner
//!   (RFC 8259 grammar, bytes of any value inside strings, nesting limited to
//!   10000 containers) and fails on trailing data;
//! * a top-level `null` succeeds and leaves the zero event; any other
//!   non-object value is an `UnmarshalTypeError`;
//! * object keys select fields exactly or by Unicode simple case folding
//!   (`ſ` folds to `s`, the Kelvin sign to `k`), the last duplicate wins, an
//!   explicit `null` leaves the field untouched, and any other non-string
//!   value for a known field is an `UnmarshalTypeError` that the reader
//!   treats like a syntax error (the line is skipped);
//! * strings are unquoted with Go's rules: invalid UTF-8 bytes and lone or
//!   unpaired `\u` surrogates each become U+FFFD.

use super::OperationEvent;

/// Go's `maxNestingDepth`: a container opened beyond this depth is a syntax
/// error.
const MAX_DEPTH: usize = 10_000;

/// Field names of the Go `Event` struct, in declaration order.
const FIELDS: [&str; 13] = [
    "ts",
    "event",
    "skill",
    "skill_version",
    "target",
    "scope",
    "mode",
    "path",
    "source_hash",
    "outcome",
    "error",
    "tool_version",
    "actor",
];

/// Outcome of decoding one non-blank log line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LineDecode {
    /// Go's `json.Unmarshal` fails, so the reader skips the line.
    Skip,
    /// Go decodes the line to this event.
    Event(Box<OperationEvent>),
}

/// Decodes one line the way Go's reader does.
pub(crate) fn decode_line(line: &[u8]) -> LineDecode {
    // Go: `len(bytes.TrimSpace(line)) == 0` skips the line before decoding.
    if is_go_blank(line) || !is_valid_json(line) {
        return LineDecode::Skip;
    }
    let mut index = skip_ws(line, 0);
    match line[index] {
        b'n' => return LineDecode::Event(Box::default()),
        b'{' => {}
        _ => return LineDecode::Skip,
    }
    let mut event = OperationEvent::default();
    index = skip_ws(line, index + 1);
    if line[index] == b'}' {
        return LineDecode::Event(Box::new(event));
    }
    loop {
        // The line is valid JSON, so every structural byte is present.
        let key_end = string_end(line, index);
        let key = unquote(&line[index + 1..key_end - 1]);
        index = skip_ws(line, key_end);
        index = skip_ws(line, index + 1); // ':'
        let value_start = index;
        let value_end = value_end(line, index);
        if let Some(slot) = field_slot(&key) {
            match line[value_start] {
                b'"' => assign(
                    &mut event,
                    slot,
                    unquote(&line[value_start + 1..value_end - 1]),
                ),
                // `null` leaves a string field alone.
                b'n' => {}
                // Any other value is an `UnmarshalTypeError`.
                _ => return LineDecode::Skip,
            }
        }
        index = skip_ws(line, value_end);
        if line[index] == b'}' {
            return LineDecode::Event(Box::new(event));
        }
        index = skip_ws(line, index + 1); // ','
    }
}

/// Go's `len(bytes.TrimSpace(line)) == 0`: only Unicode white space, decoded
/// from valid UTF-8, counts.
fn is_go_blank(line: &[u8]) -> bool {
    std::str::from_utf8(line).is_ok_and(|text| text.chars().all(char::is_whitespace))
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        index += 1;
    }
    index
}

/// Case-folded field slot of an unquoted key, as Go's field lookup does.
fn field_slot(key: &str) -> Option<usize> {
    let folded: String = key
        .chars()
        .map(|ch| match ch {
            '\u{17f}' => 's',
            '\u{212a}' => 'k',
            other => other.to_ascii_lowercase(),
        })
        .collect();
    FIELDS.iter().position(|field| *field == folded)
}

fn assign(event: &mut OperationEvent, slot: usize, value: String) {
    match slot {
        0 => event.ts = value,
        1 => event.event = value,
        2 => event.skill = value,
        3 => event.skill_version = value,
        4 => event.target = value,
        5 => event.scope = value,
        6 => event.mode = value,
        7 => event.path = value,
        8 => event.source_hash = value,
        9 => event.outcome = value,
        10 => event.error = value,
        11 => event.tool_version = value,
        _ => event.actor = value,
    }
}

/// Go's string unquoting: escapes are resolved, every invalid UTF-8 byte and
/// every unpaired surrogate escape becomes U+FFFD.
fn unquote(inner: &[u8]) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut index = 0;
    while index < inner.len() {
        match inner[index] {
            b'\\' => {
                index += 1;
                match inner[index] {
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let first = hex4(&inner[index + 1..]).unwrap_or(0);
                        index += 4;
                        out.push(decode_unicode_escape(first, inner, &mut index));
                    }
                    // `"`, `\` and `/` stand for themselves.
                    other => out.push(char::from(other)),
                }
                index += 1;
            }
            byte if byte < 0x80 => {
                out.push(char::from(byte));
                index += 1;
            }
            _ => index += push_utf8(&mut out, &inner[index..]),
        }
    }
    out
}

/// Resolves a `\uXXXX` escape (already consumed up to its last digit at
/// `index`), combining a following low-surrogate escape like
/// `utf16.DecodeRune`.
fn decode_unicode_escape(first: u32, inner: &[u8], index: &mut usize) -> char {
    if !(0xd800..0xe000).contains(&first) {
        return char::from_u32(first).unwrap_or('\u{fffd}');
    }
    let rest = &inner[*index + 1..];
    if rest.starts_with(b"\\u")
        && let Some(second) = hex4(&rest[2..])
        && (0xd800..0xdc00).contains(&first)
        && (0xdc00..0xe000).contains(&second)
    {
        *index += 6;
        let combined = 0x1_0000 + ((first - 0xd800) << 10) + (second - 0xdc00);
        return char::from_u32(combined).unwrap_or('\u{fffd}');
    }
    '\u{fffd}'
}

fn hex4(bytes: &[u8]) -> Option<u32> {
    let digits = bytes.get(..4)?;
    digits.iter().try_fold(0_u32, |acc, byte| {
        char::from(*byte)
            .to_digit(16)
            .map(|digit| (acc << 4) | digit)
    })
}

/// Pushes the rune at the start of `bytes` (or U+FFFD for one invalid byte)
/// and returns the bytes consumed, like `utf8.DecodeRune`.
fn push_utf8(out: &mut String, bytes: &[u8]) -> usize {
    let width = match bytes[0] {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    };
    if width > 0
        && let Some(slice) = bytes.get(..width)
        && let Ok(text) = std::str::from_utf8(slice)
    {
        out.push_str(text);
        return width;
    }
    out.push('\u{fffd}');
    1
}

/// Index one past the closing quote of the string starting at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    loop {
        match bytes[index] {
            b'"' => return index + 1,
            b'\\' => index += 2,
            _ => index += 1,
        }
    }
}

/// Index one past the value starting at `start` in a validated document.
fn value_end(bytes: &[u8], start: usize) -> usize {
    match bytes[start] {
        b'"' => string_end(bytes, start),
        b'{' | b'[' => {
            let mut depth = 0_usize;
            let mut index = start;
            loop {
                match bytes[index] {
                    b'"' => index = string_end(bytes, index),
                    b'{' | b'[' => {
                        depth += 1;
                        index += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            return index;
                        }
                    }
                    _ => index += 1,
                }
            }
        }
        _ => {
            let mut index = start;
            while bytes.get(index).is_some_and(|byte| {
                !matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b',' | b'}' | b']')
            }) {
                index += 1;
            }
            index
        }
    }
}

/// Go's `json.Valid`: the scanner's grammar with a 10000 container limit.
fn is_valid_json(bytes: &[u8]) -> bool {
    #[derive(Clone, Copy)]
    enum Step {
        Value,
        Key,
        After,
    }
    let mut stack: Vec<bool> = Vec::new(); // true: object, false: array
    let mut step = Step::Value;
    let mut index = skip_ws(bytes, 0);
    loop {
        match step {
            Step::Value => {
                let Some(&byte) = bytes.get(index) else {
                    return false;
                };
                match byte {
                    b'{' | b'[' => {
                        stack.push(byte == b'{');
                        if stack.len() > MAX_DEPTH {
                            return false;
                        }
                        index = skip_ws(bytes, index + 1);
                        let close = if byte == b'{' { b'}' } else { b']' };
                        if bytes.get(index) == Some(&close) {
                            stack.pop();
                            index += 1;
                            step = Step::After;
                        } else if byte == b'{' {
                            step = Step::Key;
                        }
                    }
                    b'"' => match scan_string(bytes, index) {
                        Some(end) => {
                            index = end;
                            step = Step::After;
                        }
                        None => return false,
                    },
                    b'-' | b'0'..=b'9' => match scan_number(bytes, index) {
                        Some(end) => {
                            index = end;
                            step = Step::After;
                        }
                        None => return false,
                    },
                    b't' | b'f' | b'n' => {
                        let word: &[u8] = match byte {
                            b't' => b"true",
                            b'f' => b"false",
                            _ => b"null",
                        };
                        if !bytes[index..].starts_with(word) {
                            return false;
                        }
                        index += word.len();
                        step = Step::After;
                    }
                    _ => return false,
                }
            }
            Step::Key => {
                if bytes.get(index) != Some(&b'"') {
                    return false;
                }
                let Some(end) = scan_string(bytes, index) else {
                    return false;
                };
                index = skip_ws(bytes, end);
                if bytes.get(index) != Some(&b':') {
                    return false;
                }
                index = skip_ws(bytes, index + 1);
                step = Step::Value;
            }
            Step::After => {
                index = skip_ws(bytes, index);
                let Some(&top) = stack.last() else {
                    return index == bytes.len();
                };
                match (top, bytes.get(index)) {
                    (true, Some(b',')) => {
                        index = skip_ws(bytes, index + 1);
                        step = Step::Key;
                    }
                    (false, Some(b',')) => {
                        index = skip_ws(bytes, index + 1);
                        step = Step::Value;
                    }
                    (true, Some(b'}')) | (false, Some(b']')) => {
                        stack.pop();
                        index += 1;
                    }
                    _ => return false,
                }
            }
        }
    }
}

/// End of a valid JSON string starting at `start`, or `None`.
fn scan_string(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    loop {
        match *bytes.get(index)? {
            b'"' => return Some(index + 1),
            0..=0x1f => return None,
            b'\\' => match *bytes.get(index + 1)? {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => index += 2,
                b'u' => {
                    hex4(bytes.get(index + 2..)?)?;
                    index += 6;
                }
                _ => return None,
            },
            _ => index += 1,
        }
    }
}

/// End of a valid JSON number starting at `start`, or `None`.
fn scan_number(bytes: &[u8], start: usize) -> Option<usize> {
    let digits = |mut index: usize| {
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        index
    };
    let mut index = start;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index)? {
        b'0' => index += 1,
        b'1'..=b'9' => index = digits(index),
        _ => return None,
    }
    if bytes.get(index) == Some(&b'.') {
        let end = digits(index + 1);
        if end == index + 1 {
            return None;
        }
        index = end;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let end = digits(index);
        if end == index {
            return None;
        }
        index = end;
    }
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(line: &[u8]) -> Option<OperationEvent> {
        match decode_line(line) {
            LineDecode::Event(event) => Some(*event),
            LineDecode::Skip => None,
        }
    }

    #[test]
    fn go_unmarshal_rules_for_known_fields() {
        let event =
            decode(r#"{"TS":"a","ts":"b","ſkill":"x","tarGet":"t","outcome":null}"#.as_bytes())
                .expect("decodes");
        assert_eq!(event.ts, "b");
        assert_eq!(event.skill, "x");
        assert_eq!(event.target, "t");
        assert_eq!(event.outcome, "");
        assert_eq!(decode(b"null"), Some(OperationEvent::default()));
        for skipped in [
            &b"[1]"[..],
            b"\"s\"",
            b"7",
            b"true",
            b"{\"ts\":5}",
            b"{\"ts\":{}}",
            b"{\"ts\":[\"a\"]}",
            b"{\"ts\":\"a\"} x",
            b"{\"ts\":\"a\",}",
            b"{\"a\":01}",
            b"{\"ts\":\"a\\x\"}",
            b"{\"ts\":\"a\nb\"}",
            b"\xc2\xa0{}",
        ] {
            assert!(
                decode(skipped).is_none(),
                "{}",
                String::from_utf8_lossy(skipped)
            );
        }
        // Unknown members of any shape are ignored.
        assert!(decode(br#"{"x":[{"y":1}],"ts":"a"}"#).is_some());
    }

    #[test]
    fn invalid_utf8_and_surrogates_become_replacement_characters() {
        let event = decode(b"{\"ts\":\"\xff\xed\xa0\x80\xe3\x81\"}").expect("decodes");
        assert_eq!(event.ts, "\u{fffd}".repeat(6));
        let event =
            decode(br#"{"ts":"\ud800","event":"\ud83d\ude00","skill":"\udc00x\ud800\u0041"}"#)
                .expect("decodes");
        assert_eq!(event.ts, "\u{fffd}");
        assert_eq!(event.event, "\u{1f600}");
        assert_eq!(event.skill, "\u{fffd}x\u{fffd}A");
    }

    #[test]
    fn nesting_limit_matches_the_go_scanner() {
        let nested = |depth: usize| {
            format!(
                "{{\"x\":{}{}}}",
                "[".repeat(depth - 1),
                "]".repeat(depth - 1)
            )
            .into_bytes()
        };
        assert!(decode(&nested(MAX_DEPTH)).is_some());
        assert!(decode(&nested(MAX_DEPTH + 1)).is_none());
    }
}
