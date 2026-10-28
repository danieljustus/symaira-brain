//! Go-exact decoding of one operation-log line.
//!
//! The Go reader (`events.Logger.Read`) calls `json.Unmarshal` per line into
//! an `Event` and skips the line on any error. `encoding/json` and a derived
//! serde struct disagree on several inputs that a hand-edited or torn log can
//! contain: case-insensitive keys, duplicate keys, `null` values, a top-level
//! `null` or array, lone surrogates and invalid UTF-8 (both become U+FFFD in
//! Go) and numbers or nesting that only one side accepts. This module decides
//! only the lines whose Go outcome it can state with certainty and reports
//! everything else as [`LineDecode::Unsure`], which the caller must hand to
//! the Go implementation.

use super::OperationEvent;

/// Nesting depth above which the scan gives up (Go allows 10000, serde 128).
const MAX_DEPTH: usize = 200;

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
    /// Go's outcome cannot be stated with certainty; use the Go reader.
    Unsure,
}

enum Fail {
    Invalid,
    Deep,
}

/// Byte ranges of one top-level object member.
struct Member {
    key: (usize, usize),
    value: (usize, usize),
}

/// Decodes one line the way Go's reader does, or reports uncertainty.
pub(crate) fn decode_line(line: &[u8]) -> LineDecode {
    // Go: `len(bytes.TrimSpace(line)) == 0` skips the line before decoding.
    // `\v` and `\f` are Unicode space for Go but not JSON whitespace.
    if line
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r'))
    {
        return LineDecode::Skip;
    }
    let Ok(text) = std::str::from_utf8(line) else {
        return LineDecode::Unsure;
    };
    let bytes = text.as_bytes();
    let mut index = 0;
    skip_ws(bytes, &mut index);
    match bytes.get(index) {
        Some(b'{') => {}
        Some(_) => return scalar_or_array(bytes, index),
        None => return LineDecode::Skip,
    }
    let mut members = Vec::new();
    match parse_object(bytes, &mut index, 1, Some(&mut members)) {
        Ok(()) => {}
        Err(Fail::Invalid) => return LineDecode::Skip,
        Err(Fail::Deep) => return LineDecode::Unsure,
    }
    skip_ws(bytes, &mut index);
    if index != bytes.len() {
        return LineDecode::Skip;
    }
    build_event(text, &members)
}

/// A top-level value that is not an object: Go fails with an
/// `UnmarshalTypeError` (or a syntax error), except for the literal `null`,
/// which succeeds and yields a zero event.
fn scalar_or_array(bytes: &[u8], start: usize) -> LineDecode {
    let mut index = start;
    match skip_value(bytes, &mut index, 1) {
        Err(Fail::Invalid) => return LineDecode::Skip,
        Err(Fail::Deep) => return LineDecode::Unsure,
        Ok(()) => {}
    }
    skip_ws(bytes, &mut index);
    if index != bytes.len() {
        return LineDecode::Skip;
    }
    if bytes[start..].starts_with(b"null") {
        LineDecode::Unsure
    } else {
        LineDecode::Skip
    }
}

fn build_event(text: &str, members: &[Member]) -> LineDecode {
    let mut event = OperationEvent::default();
    let mut seen = [false; FIELDS.len()];
    for member in members {
        let Ok(key) = serde_json::from_str::<String>(&text[member.key.0..member.key.1]) else {
            return LineDecode::Unsure;
        };
        if let Some(slot) = FIELDS.iter().position(|field| *field == key) {
            if seen[slot] {
                return LineDecode::Unsure;
            }
            seen[slot] = true;
            let raw = &text[member.value.0..member.value.1];
            if !raw.starts_with('"') {
                return LineDecode::Unsure;
            }
            let Ok(value) = serde_json::from_str::<String>(raw) else {
                return LineDecode::Unsure;
            };
            assign(&mut event, slot, value);
        } else if !key.is_ascii() || FIELDS.iter().any(|field| field.eq_ignore_ascii_case(&key)) {
            // Go folds case (and the long s / Kelvin sign) when matching keys.
            return LineDecode::Unsure;
        }
    }
    LineDecode::Event(Box::new(event))
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

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *index += 1;
    }
}

fn skip_value(bytes: &[u8], index: &mut usize, depth: usize) -> Result<(), Fail> {
    if depth > MAX_DEPTH {
        return Err(Fail::Deep);
    }
    match bytes.get(*index) {
        Some(b'{') => parse_object(bytes, index, depth, None),
        Some(b'[') => parse_array(bytes, index, depth),
        Some(b'"') => skip_string(bytes, index),
        Some(b'-' | b'0'..=b'9') => skip_number(bytes, index),
        Some(b't') => literal(bytes, index, b"true"),
        Some(b'f') => literal(bytes, index, b"false"),
        Some(b'n') => literal(bytes, index, b"null"),
        _ => Err(Fail::Invalid),
    }
}

fn parse_object(
    bytes: &[u8],
    index: &mut usize,
    depth: usize,
    mut members: Option<&mut Vec<Member>>,
) -> Result<(), Fail> {
    if depth > MAX_DEPTH {
        return Err(Fail::Deep);
    }
    *index += 1;
    skip_ws(bytes, index);
    if bytes.get(*index) == Some(&b'}') {
        *index += 1;
        return Ok(());
    }
    loop {
        skip_ws(bytes, index);
        if bytes.get(*index) != Some(&b'"') {
            return Err(Fail::Invalid);
        }
        let key_start = *index;
        skip_string(bytes, index)?;
        let key = (key_start, *index);
        skip_ws(bytes, index);
        if bytes.get(*index) != Some(&b':') {
            return Err(Fail::Invalid);
        }
        *index += 1;
        skip_ws(bytes, index);
        let value_start = *index;
        skip_value(bytes, index, depth + 1)?;
        if let Some(members) = members.as_deref_mut() {
            members.push(Member {
                key,
                value: (value_start, *index),
            });
        }
        skip_ws(bytes, index);
        match bytes.get(*index) {
            Some(b',') => *index += 1,
            Some(b'}') => {
                *index += 1;
                return Ok(());
            }
            _ => return Err(Fail::Invalid),
        }
    }
}

fn parse_array(bytes: &[u8], index: &mut usize, depth: usize) -> Result<(), Fail> {
    *index += 1;
    skip_ws(bytes, index);
    if bytes.get(*index) == Some(&b']') {
        *index += 1;
        return Ok(());
    }
    loop {
        skip_ws(bytes, index);
        skip_value(bytes, index, depth + 1)?;
        skip_ws(bytes, index);
        match bytes.get(*index) {
            Some(b',') => *index += 1,
            Some(b']') => {
                *index += 1;
                return Ok(());
            }
            _ => return Err(Fail::Invalid),
        }
    }
}

fn skip_string(bytes: &[u8], index: &mut usize) -> Result<(), Fail> {
    *index += 1;
    loop {
        match bytes.get(*index) {
            None | Some(0..=0x1F) => return Err(Fail::Invalid),
            Some(b'"') => {
                *index += 1;
                return Ok(());
            }
            Some(b'\\') => {
                *index += 1;
                match bytes.get(*index) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => *index += 1,
                    Some(b'u') => {
                        *index += 1;
                        for _ in 0..4 {
                            if !bytes.get(*index).is_some_and(u8::is_ascii_hexdigit) {
                                return Err(Fail::Invalid);
                            }
                            *index += 1;
                        }
                    }
                    _ => return Err(Fail::Invalid),
                }
            }
            Some(_) => *index += 1,
        }
    }
}

fn skip_number(bytes: &[u8], index: &mut usize) -> Result<(), Fail> {
    if bytes.get(*index) == Some(&b'-') {
        *index += 1;
    }
    match bytes.get(*index) {
        Some(b'0') => *index += 1,
        Some(b'1'..=b'9') => digits(bytes, index),
        _ => return Err(Fail::Invalid),
    }
    if bytes.get(*index) == Some(&b'.') {
        *index += 1;
        if !bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            return Err(Fail::Invalid);
        }
        digits(bytes, index);
    }
    if matches!(bytes.get(*index), Some(b'e' | b'E')) {
        *index += 1;
        if matches!(bytes.get(*index), Some(b'+' | b'-')) {
            *index += 1;
        }
        if !bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            return Err(Fail::Invalid);
        }
        digits(bytes, index);
    }
    Ok(())
}

fn digits(bytes: &[u8], index: &mut usize) {
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
}

fn literal(bytes: &[u8], index: &mut usize, word: &[u8]) -> Result<(), Fail> {
    if bytes[*index..].starts_with(word) {
        *index += word.len();
        Ok(())
    } else {
        Err(Fail::Invalid)
    }
}
