//! `encoding/json` decoding (Go 1.27, where it runs on the v2 engine with v1
//! semantics) of the two shapes the memory store hydrates: `map[string]string`
//! (metadata) and `[]float32` (embeddings), with the diagnostics Go prints.
//!
//! Go validates the whole document first and only then decodes, remembering
//! the first type mismatch while it keeps going. Syntax errors therefore win
//! over type errors, a type error never stops the walk, and `null` leaves the
//! target untouched (a nil map or slice). The syntax walk mirrors
//! `jsontext.Decoder.ReadValue` + `jsonwire` byte for byte.

use std::collections::BTreeMap;

use crate::slog::quote_bytes;

/// Why the validator stopped.
enum Stop {
    /// `io.ErrUnexpectedEOF`.
    End,
    /// A rendered diagnostic.
    Text(String),
}

type Walk<T> = Result<T, Stop>;

const MAX_DEPTH: usize = 10_000;

fn decode_rune(bytes: &[u8]) -> (char, usize, bool) {
    let window = &bytes[..bytes.len().min(4)];
    let valid = match std::str::from_utf8(window) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&window[..error.valid_up_to()]).unwrap_or(""),
    };
    valid
        .chars()
        .next()
        .map_or(('\u{fffd}', 1, true), |c| (c, c.len_utf8(), false))
}

fn is_format(c: char) -> bool {
    matches!(u32::from(c),
        0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x180e | 0x200b..=0x200f
        | 0x202a..=0x202e | 0x2060..=0x2064 | 0x2066..=0x206f | 0xfeff
        | 0xfff9..=0xfffb | 0xe0001 | 0xe0020..=0xe007f
        | 0xe000..=0xf8ff | 0xf0000..=0x10ffff)
}

/// `unicode.IsPrint` (approximated for the non-ASCII tail).
fn is_print(c: char) -> bool {
    if c.is_ascii() {
        return (' '..='~').contains(&c);
    }
    !(c.is_control() || c.is_whitespace() || is_format(c))
}

/// `strconv.QuoteRune`.
fn quote_rune(c: char) -> String {
    let body = match c {
        '\'' => "\\'".to_owned(),
        '\\' => "\\\\".to_owned(),
        _ if is_print(c) => c.to_string(),
        '\u{7}' => "\\a".to_owned(),
        '\u{8}' => "\\b".to_owned(),
        '\u{c}' => "\\f".to_owned(),
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        '\u{b}' => "\\v".to_owned(),
        _ if u32::from(c) < 0x20 || u32::from(c) == 0x7f => format!("\\x{:02x}", u32::from(c)),
        _ if u32::from(c) < 0x10000 => format!("\\u{:04x}", u32::from(c)),
        _ => format!("\\U{:08x}", u32::from(c)),
    };
    format!("'{body}'")
}

/// `jsonwire.QuoteRune`.
fn quote_first_rune(bytes: &[u8]) -> String {
    let (c, width, invalid) = decode_rune(bytes);
    if invalid && width == 1 {
        return format!("'\\x{:x}'", bytes[0]);
    }
    quote_rune(c)
}

/// `NewInvalidCharacterError(rest, where)`.
fn invalid_character(rest: &[u8], location: &str) -> Stop {
    Stop::Text(format!(
        "invalid character {} {location}",
        quote_first_rune(rest)
    ))
}

/// `NewInvalidEscapeSequenceError(what)`.
fn invalid_escape(what: &[u8]) -> Stop {
    let label = if what.len() > 6 {
        "surrogate pair"
    } else {
        "escape sequence"
    };
    let runes: usize = what
        .utf8_chunks()
        .map(|chunk| chunk.valid().chars().count() + chunk.invalid().len())
        .sum();
    let need_escape = what.utf8_chunks().any(|chunk| {
        !chunk.invalid().is_empty()
            || chunk
                .valid()
                .chars()
                .any(|c| c == '`' || c == '\u{fffd}' || c.is_whitespace() || !is_print(c))
    });
    let rendered = if runes == 1 {
        quote_first_rune(what)
    } else if need_escape {
        quote_bytes(what)
    } else {
        format!("`{}`", String::from_utf8_lossy(what))
    };
    Stop::Text(format!("invalid {label} {rendered} in string"))
}

fn skip_space(b: &[u8], mut pos: usize) -> usize {
    while pos < b.len() && matches!(b[pos], b' ' | b'\t' | b'\r' | b'\n') {
        pos += 1;
    }
    pos
}

fn literal(b: &[u8], pos: usize, word: &str) -> Walk<usize> {
    let word_bytes = word.as_bytes();
    let rest = &b[pos..];
    for (index, expected) in word_bytes.iter().enumerate() {
        if index >= rest.len() {
            break;
        }
        if rest[index] != *expected {
            return Err(invalid_character(
                &rest[index..],
                &format!(
                    "in literal {word} (expecting {})",
                    quote_rune(char::from(*expected))
                ),
            ));
        }
    }
    if rest.len() < word_bytes.len() {
        return Err(Stop::End);
    }
    Ok(pos + word_bytes.len())
}

fn hex4(b: &[u8]) -> Option<u16> {
    if b.len() != 4 {
        return None;
    }
    let mut value = 0_u16;
    for byte in b {
        let digit = char::from(*byte).to_digit(16)?;
        value = (value << 4) | u16::try_from(digit).ok()?;
    }
    Some(value)
}

/// `hasEscapedUTF16Prefix` with `lowerSurrogateHalf == false`.
fn has_escaped_prefix(b: &[u8]) -> bool {
    for (index, c) in b.iter().enumerate() {
        match index {
            0 if *c != b'\\' => return false,
            1 if *c != b'u' => return false,
            2..=5 if !c.is_ascii_hexdigit() => return false,
            _ => {}
        }
    }
    true
}

/// `utf8.FullRune` for a buffer that starts with an invalid sequence.
fn truncated_rune(b: &[u8]) -> bool {
    std::str::from_utf8(b).is_err_and(|error| error.error_len().is_none())
}

/// `ConsumeStringResumable` with `validateUTF8 == false`.
fn string(b: &[u8], pos: usize) -> Walk<usize> {
    if pos >= b.len() {
        return Err(Stop::End);
    }
    if b[pos] != b'"' {
        return Err(invalid_character(
            &b[pos..],
            "at start of string (expecting '\"')",
        ));
    }
    let mut n = pos + 1;
    while n < b.len() {
        while n < b.len() && b[n] < 0x80 && b[n] >= b' ' && b[n] != b'\\' && b[n] != b'"' {
            n += 1;
        }
        if n >= b.len() {
            return Err(Stop::End);
        }
        if b[n] == b'"' {
            return Ok(n + 1);
        }
        let (c, width, invalid) = decode_rune(&b[n..]);
        if width > 1 {
            n += width;
        } else if c == '\\' && !invalid {
            if b.len() < n + 2 {
                return Err(Stop::End);
            }
            match b[n + 1] {
                b'/' | b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't' => n += 2,
                b'u' => {
                    if b.len() < n + 6 {
                        if has_escaped_prefix(&b[n..]) {
                            return Err(Stop::End);
                        }
                        return Err(invalid_escape(&b[n..]));
                    }
                    if hex4(&b[n + 2..n + 6]).is_none() {
                        return Err(invalid_escape(&b[n..n + 6]));
                    }
                    n += 6;
                }
                _ => return Err(invalid_escape(&b[n..n + 2])),
            }
        } else if invalid {
            // A truncated rune ends the input; an invalid byte is accepted
            // because the validator runs with AllowInvalidUTF8.
            if truncated_rune(&b[n..]) {
                return Err(Stop::End);
            }
            n += 1;
        } else {
            return Err(invalid_character(
                &b[n..],
                "in string (expecting non-control character)",
            ));
        }
    }
    Err(Stop::End)
}

/// `ConsumeNumber`: the longest valid prefix, or the diagnostic.
fn number(b: &[u8], pos: usize) -> Walk<usize> {
    let digits = |mut n: usize| {
        while n < b.len() && b[n].is_ascii_digit() {
            n += 1;
        }
        n
    };
    let mut n = pos;
    if b[n] == b'-' {
        n += 1;
    }
    if n >= b.len() {
        return Err(Stop::End);
    }
    match b[n] {
        b'0' => n += 1,
        b'1'..=b'9' => n = digits(n + 1),
        _ => return Err(invalid_character(&b[n..], "in number (expecting digit)")),
    }
    if n < b.len() && b[n] == b'.' {
        n += 1;
        if n >= b.len() {
            return Err(Stop::End);
        }
        if !b[n].is_ascii_digit() {
            return Err(invalid_character(&b[n..], "in number (expecting digit)"));
        }
        n = digits(n + 1);
    }
    if n < b.len() && (b[n] == b'e' || b[n] == b'E') {
        n += 1;
        if n < b.len() && (b[n] == b'-' || b[n] == b'+') {
            n += 1;
        }
        if n >= b.len() {
            return Err(Stop::End);
        }
        if !b[n].is_ascii_digit() {
            return Err(invalid_character(&b[n..], "in number (expecting digit)"));
        }
        n = digits(n + 1);
    }
    Ok(n)
}

fn value(b: &[u8], pos: usize, depth: usize) -> Walk<usize> {
    if pos >= b.len() {
        return Err(Stop::End);
    }
    match b[pos] {
        b'n' => literal(b, pos, "null"),
        b'f' => literal(b, pos, "false"),
        b't' => literal(b, pos, "true"),
        b'"' => string(b, pos),
        b'-' | b'0'..=b'9' => number(b, pos),
        b'{' => object(b, pos, depth),
        b'[' => array(b, pos, depth),
        _ => Err(invalid_character(&b[pos..], "at start of value")),
    }
}

fn object(b: &[u8], mut pos: usize, depth: usize) -> Walk<usize> {
    if depth == MAX_DEPTH + 1 {
        return Err(Stop::Text("exceeded max depth".into()));
    }
    pos = skip_space(b, pos + 1);
    if pos >= b.len() {
        return Err(Stop::End);
    }
    if b[pos] == b'}' {
        return Ok(pos + 1);
    }
    loop {
        pos = skip_space(b, pos);
        pos = string(b, pos)?;
        pos = skip_space(b, pos);
        if pos >= b.len() {
            return Err(Stop::End);
        }
        if b[pos] != b':' {
            return Err(invalid_character(
                &b[pos..],
                "after object name (expecting ':')",
            ));
        }
        pos = skip_space(b, pos + 1);
        pos = value(b, pos, depth + 1)?;
        pos = skip_space(b, pos);
        if pos >= b.len() {
            return Err(Stop::End);
        }
        match b[pos] {
            b',' => pos += 1,
            b'}' => return Ok(pos + 1),
            _ => {
                return Err(invalid_character(
                    &b[pos..],
                    "after object value (expecting ',' or '}')",
                ));
            }
        }
    }
}

fn array(b: &[u8], mut pos: usize, depth: usize) -> Walk<usize> {
    if depth == MAX_DEPTH + 1 {
        return Err(Stop::Text("exceeded max depth".into()));
    }
    pos = skip_space(b, pos + 1);
    if pos >= b.len() {
        return Err(Stop::End);
    }
    if b[pos] == b']' {
        return Ok(pos + 1);
    }
    loop {
        pos = skip_space(b, pos);
        pos = value(b, pos, depth + 1)?;
        pos = skip_space(b, pos);
        if pos >= b.len() {
            return Err(Stop::End);
        }
        match b[pos] {
            b',' => pos += 1,
            b']' => return Ok(pos + 1),
            _ => {
                return Err(invalid_character(
                    &b[pos..],
                    "after array element (expecting ',' or ']')",
                ));
            }
        }
    }
}

/// `transformSyntacticError`: drops the "(expecting ...)" hint (except for
/// literals) and applies the historical wording.
fn transform(message: &str) -> String {
    let mut text = message;
    if let Some(index) = text.find(" (expecting")
        && !text.contains(" in literal")
    {
        text = &text[..index];
    }
    const PAIRS: [(&str, &str); 5] = [
        ("object name", "object key"),
        ("at start of value", "looking for beginning of value"),
        (
            "at start of string",
            "looking for beginning of object key string",
        ),
        ("after object value", "after object key:value pair"),
        ("in number", "in numeric literal"),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while !rest.is_empty() {
        for (old, new) in PAIRS {
            if let Some(tail) = rest.strip_prefix(old) {
                out.push_str(new);
                rest = tail;
                continue 'scan;
            }
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        out.push_str(&rest[..width]);
        rest = &rest[width..];
    }
    out
}

/// Walks the top-level value. The scanner recurses once per nesting level and
/// the shipped limit is 10 000 levels, so a document long enough to nest that
/// deep runs on a thread whose stack is sized for it; the ordinary stack
/// overflows (an abort, not an error) in unoptimised builds.
fn walk_value(data: &[u8], start: usize, depth: usize) -> Walk<usize> {
    const DEEP_STACK: usize = 256 * 1024 * 1024;
    if data.len() <= MAX_DEPTH / 8 {
        return value(data, start, depth);
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(DEEP_STACK)
            .spawn_scoped(scope, || value(data, start, depth))
            .map_or_else(
                |_| Err(Stop::Text("exceeded max depth".into())),
                |handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err(Stop::Text("exceeded max depth".into())))
                },
            )
    })
}

/// `json.Valid`'s error: the first syntax error of the document.
///
/// # Errors
/// Returns Go's diagnostic text.
pub fn check_valid(data: &[u8]) -> Result<(), String> {
    let finish = |stop: Stop| match stop {
        Stop::End => "unexpected end of JSON input".to_owned(),
        Stop::Text(text) => transform(&text),
    };
    let start = skip_space(data, 0);
    let end = walk_value(data, start, 1).map_err(finish)?;
    let tail = skip_space(data, end);
    if tail < data.len() {
        return Err(finish(invalid_character(
            &data[tail..],
            "after top-level value",
        )));
    }
    Ok(())
}

/// The kind word `UnmarshalTypeError` prints.
fn kind(first: u8) -> &'static str {
    match first {
        b'{' => "object",
        b'[' => "array",
        b'"' => "string",
        b't' | b'f' => "bool",
        _ => "number",
    }
}

/// Decodes a validated string literal; invalid UTF-8 and lone surrogates
/// become U+FFFD like `AppendUnquote`.
fn unquote(b: &[u8], pos: usize, end: usize) -> String {
    let body = &b[pos + 1..end - 1];
    let mut out: Vec<u8> = Vec::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if byte != b'\\' {
            out.push(byte);
            index += 1;
            continue;
        }
        match body[index + 1] {
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'/' => out.push(b'/'),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            _ => {
                let high = hex4(&body[index + 2..index + 6]).unwrap_or(0xfffd);
                index += 6;
                let character = if (0xd800..0xdc00).contains(&high) {
                    let pair = body
                        .get(index..index + 6)
                        .filter(|rest| rest[0] == b'\\' && rest[1] == b'u')
                        .and_then(|rest| hex4(&rest[2..6]))
                        .filter(|low| (0xdc00..0xe000).contains(low));
                    pair.and_then(|low| {
                        index += 6;
                        char::from_u32(
                            0x10000
                                + ((u32::from(high) - 0xd800) << 10)
                                + (u32::from(low) - 0xdc00),
                        )
                    })
                } else {
                    char::from_u32(u32::from(high))
                };
                let mut buffer = [0_u8; 4];
                out.extend_from_slice(
                    character
                        .unwrap_or('\u{fffd}')
                        .encode_utf8(&mut buffer)
                        .as_bytes(),
                );
                continue;
            }
        }
        index += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `UnmarshalTypeError.Error()` for a value at `path` of Go type `target`.
fn type_error(found: &str, path: &str, target: &str) -> String {
    let last = path.rsplit('.').next().unwrap_or("");
    let numeric = !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit());
    let what = if numeric { "" } else { "Go struct field " };
    format!("json: cannot unmarshal {found} into {what}.{path} of type {target}")
}

fn top_error(found: &str, target: &str) -> String {
    format!("json: cannot unmarshal {found} into Go value of type {target}")
}

/// Decodes into a `map[string]string`. `Ok(None)` is a nil map (JSON null).
///
/// # Errors
/// Returns the diagnostic `json.Unmarshal` returns.
pub fn decode_string_map(data: &[u8]) -> Result<Option<BTreeMap<String, String>>, String> {
    const TARGET: &str = "map[string]string";
    check_valid(data)?;
    let start = skip_space(data, 0);
    match data[start] {
        b'n' => return Ok(None),
        b'{' => {}
        other => return Err(top_error(kind(other), TARGET)),
    }
    let mut map = BTreeMap::new();
    let mut saved: Option<String> = None;
    let mut pos = skip_space(data, start + 1);
    if data[pos] == b'}' {
        return Ok(Some(map));
    }
    loop {
        pos = skip_space(data, pos);
        let key_end = string(data, pos).unwrap_or(pos);
        let key = unquote(data, pos, key_end);
        pos = skip_space(data, key_end) + 1;
        pos = skip_space(data, pos);
        let value_end = walk_value(data, pos, 2).unwrap_or(pos);
        let entry = match data[pos] {
            b'"' => unquote(data, pos, value_end),
            b'n' => String::new(),
            other => {
                saved.get_or_insert_with(|| type_error(kind(other), &key, "string"));
                String::new()
            }
        };
        map.insert(key, entry);
        pos = skip_space(data, value_end);
        if data[pos] == b',' {
            pos += 1;
        } else {
            break;
        }
    }
    saved.map_or(Ok(Some(map)), Err)
}

/// Decodes into a `[]float32`. `Ok(None)` is a nil slice (JSON null).
///
/// # Errors
/// Returns the diagnostic `json.Unmarshal` returns.
pub fn decode_f32_vec(data: &[u8]) -> Result<Option<Vec<f32>>, String> {
    const TARGET: &str = "[]float32";
    check_valid(data)?;
    let start = skip_space(data, 0);
    match data[start] {
        b'n' => return Ok(None),
        b'[' => {}
        other => return Err(top_error(kind(other), TARGET)),
    }
    let mut values = Vec::new();
    let mut saved: Option<String> = None;
    let mut pos = skip_space(data, start + 1);
    if data[pos] == b']' {
        return Ok(Some(values));
    }
    loop {
        pos = skip_space(data, pos);
        let end = walk_value(data, pos, 2).unwrap_or(pos);
        let index = values.len().to_string();
        match data[pos] {
            b'-' | b'0'..=b'9' => {
                let text = std::str::from_utf8(&data[pos..end]).unwrap_or("0");
                match text.parse::<f32>() {
                    Ok(parsed) if parsed.is_finite() => values.push(parsed),
                    _ => {
                        saved.get_or_insert_with(|| {
                            type_error(&format!("number {text}"), &index, "float32")
                        });
                        values.push(0.0);
                    }
                }
            }
            b'n' => values.push(0.0),
            other => {
                saved.get_or_insert_with(|| type_error(kind(other), &index, "float32"));
                values.push(0.0);
            }
        }
        pos = skip_space(data, end);
        if data[pos] == b',' {
            pos += 1;
        } else {
            break;
        }
    }
    saved.map_or(Ok(Some(values)), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values were produced by the Go 1.27.3 toolchain.
    #[test]
    fn syntax_diagnostics_match_go() {
        let cases: &[(&[u8], &str)] = &[
            (
                b"broken",
                "invalid character 'b' looking for beginning of value",
            ),
            (b"", "unexpected end of JSON input"),
            (b"{\"a\":", "unexpected end of JSON input"),
            (b"tru", "unexpected end of JSON input"),
            (b"{} x", "invalid character 'x' after top-level value"),
            (
                b"{1:2}",
                "invalid character '1' looking for beginning of object key string",
            ),
            (b"{\"a\" 1}", "invalid character '1' after object key"),
            (b"[1 2]", "invalid character '2' after array element"),
            (b"\"\\x\"", "invalid escape sequence `\\x` in string"),
            (b"-", "unexpected end of JSON input"),
            (
                "é".as_bytes(),
                "invalid character 'é' looking for beginning of value",
            ),
            (
                b"{\"a\":nul}",
                "invalid character '}' in literal null (expecting 'l')",
            ),
            (
                b"[trux]",
                "invalid character 'x' in literal true (expecting 'e')",
            ),
            (b"[01]", "invalid character '1' after array element"),
            (b"[1.e3]", "invalid character 'e' in numeric literal"),
            (
                b"{\"a\":\"\\u12\"}",
                "invalid escape sequence `\\u12\"}` in string",
            ),
            (
                b"[\"\\uZZZZ\"]",
                "invalid escape sequence `\\uZZZZ` in string",
            ),
            (
                b"{'a':1}",
                "invalid character '\\'' looking for beginning of object key string",
            ),
            (b"{\"a\":\"\x01\"}", "invalid character '\\x01' in string"),
            (
                b"\xc2\xa0[1]",
                "invalid character '\\u00a0' looking for beginning of value",
            ),
            (
                b"{\"a\":\"b\"; }",
                "invalid character ';' after object key:value pair",
            ),
            (
                b"{\"a\":\"b\"}\0",
                "invalid character '\\x00' after top-level value",
            ),
            (
                b"{\"a\":1,\"b\":}",
                "invalid character '}' looking for beginning of value",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                check_valid(input).expect_err("invalid"),
                *expected,
                "{input:?}"
            );
        }
        let deep = "[".repeat(10_001);
        assert_eq!(
            check_valid(deep.as_bytes()).unwrap_err(),
            "exceeded max depth"
        );
        assert!(check_valid(b"{\"a\":\"b\xff\",\"c\":\"\\ud800\"}").is_ok());
    }

    #[test]
    fn type_errors_follow_the_first_mismatch() {
        let map_error = |text: &str| decode_string_map(text.as_bytes()).expect_err("must fail");
        assert_eq!(
            map_error("[]"),
            "json: cannot unmarshal array into Go value of type map[string]string"
        );
        assert_eq!(
            map_error("{\"n\":1}"),
            "json: cannot unmarshal number into Go struct field .n of type string"
        );
        assert_eq!(
            map_error("{\"a\":1,\"b\":true}"),
            "json: cannot unmarshal number into Go struct field .a of type string"
        );
        assert_eq!(
            map_error("{\"0\":1}"),
            "json: cannot unmarshal number into .0 of type string"
        );
        assert_eq!(
            map_error("{\"a.b\":1}"),
            "json: cannot unmarshal number into Go struct field .a.b of type string"
        );
        assert!(decode_string_map(b"null").unwrap().is_none());
        assert_eq!(
            decode_string_map(b"{\"n\":null}").unwrap().unwrap()["n"],
            ""
        );
        assert_eq!(
            decode_f32_vec(b"[1e39]").unwrap_err(),
            "json: cannot unmarshal number 1e39 into .0 of type float32"
        );
        assert_eq!(
            decode_f32_vec(b"[1,\"x\"]").unwrap_err(),
            "json: cannot unmarshal string into .1 of type float32"
        );
        assert_eq!(decode_f32_vec(b"[null]").unwrap().unwrap(), vec![0.0]);
        assert_eq!(
            decode_f32_vec(b"{}").unwrap_err(),
            "json: cannot unmarshal object into Go value of type []float32"
        );
    }

    #[test]
    fn strings_resolve_escapes_and_invalid_utf8() {
        let map = decode_string_map(b"{\"k\\u00e9\":\"\\ud83d\\ude00 \xff\\ud800\"}")
            .unwrap()
            .unwrap();
        assert_eq!(map["k\u{e9}"], "\u{1f600} \u{fffd}\u{fffd}");
    }
}
