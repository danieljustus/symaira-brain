//! `BurntSushi/toml` v1.6.0 syntax diagnostics as surfaced by corekit's `configkit`.
//!
//! `toml_edit` decides whether a document is syntactically valid (both follow
//! TOML 1.1). When it rejects a document, the message is reconstructed in the
//! Go parser's wording for the statement shapes whose Go output was measured
//! against the real loader (see `setup_config_toml_tests.rs`). Any other shape
//! keeps the Go envelope (`toml: line N: ...`) around the `toml_edit` text,
//! because the exact Go lexer message is unknown; that residual and the two
//! documents the dialects disagree on are recorded in the oracle README.
use symbrain_core::config::format_go_quoted_bytes;
use toml_edit::DocumentMut;

#[path = "setup_config_toml_value.rs"]
mod value;

/// Parses a config file the way `toml.DecodeFile` decides validity.
pub(super) fn parse(bytes: &[u8]) -> Result<DocumentMut, String> {
    let mut data = bytes;
    if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        data = &data[2..];
    } else if data.starts_with(&[0xef, 0xbb, 0xbf]) {
        data = &data[3..];
    }
    if data.iter().take(6).any(|byte| *byte == 0) {
        return Err("toml: line 1: files cannot contain NULL bytes; probably using UTF-16; TOML files must be UTF-8".into());
    }
    let text = match std::str::from_utf8(data) {
        Ok(text) => text,
        Err(error) => return Err(invalid_utf8(data, error.valid_up_to())),
    };
    text.parse::<DocumentMut>().map_err(|error| {
        let offset = error.span().map_or(text.len(), |span| span.start);
        diagnose(text, offset, error.message())
    })
}

fn invalid_utf8(data: &[u8], offset: usize) -> String {
    let line = data[..offset].split(|byte| *byte == b'\n').count();
    let start = data[..offset]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let prefix = String::from_utf8_lossy(&data[start..offset]);
    let message = format!("invalid UTF-8 byte: 0x{:x}", data[offset]);
    // A byte after `=` is lexed as part of a value, so the key is known.
    if let Some(index) = prefix.find('=') {
        let (_, segments) = scan_key(&prefix[..index]);
        return envelope(line, Some(&segments.join(".")), &message);
    }
    envelope(line, None, &message)
}

fn envelope(line: usize, last_key: Option<&str>, message: &str) -> String {
    match last_key.filter(|key| !key.is_empty()) {
        Some(key) => format!(
            "toml: line {line} (last key {}): {message}",
            format_go_quoted_bytes(key.as_bytes())
        ),
        None => format!("toml: line {line}: {message}"),
    }
}

/// Go `%q` for a rune, as used inside the lexer messages.
fn quote_rune(character: Option<char>) -> String {
    match character {
        None => "'\\x00'".into(),
        Some('\n') => "'\\n'".into(),
        Some('\t') => "'\\t'".into(),
        Some('\r') => "'\\r'".into(),
        Some('\'') => "'\\''".into(),
        Some('\\') => "'\\\\'".into(),
        Some(c) if c.is_control() => format!("'\\x{:02x}'", c as u32),
        Some(c) => format!("'{c}'"),
    }
}

/// Splits `text` into raw key segments up to the first unparsable character.
/// Returns the byte offset where scanning stopped.
fn scan_key(text: &str) -> (usize, Vec<String>) {
    let bytes = text.as_bytes();
    let blank = |position: usize| matches!(bytes.get(position), Some(b' ' | b'\t'));
    let mut segments = Vec::new();
    let mut position = 0;
    loop {
        while blank(position) {
            position += 1;
        }
        let start = position;
        if matches!(bytes.get(position), Some(b'"' | b'\'')) {
            let quote = bytes[position];
            position += 1;
            let begin = position;
            while position < bytes.len() && bytes[position] != quote {
                position += 1;
            }
            if position >= bytes.len() {
                return (start, segments);
            }
            segments.push(text[begin..position].to_owned());
            position += 1;
        } else {
            while position < bytes.len()
                && (bytes[position].is_ascii_alphanumeric()
                    || matches!(bytes[position], b'_' | b'-'))
            {
                position += 1;
            }
            if position == start {
                return (start, segments);
            }
            segments.push(text[start..position].to_owned());
        }
        while blank(position) {
            position += 1;
        }
        if bytes.get(position) == Some(&b'.') {
            position += 1;
        } else {
            return (position, segments);
        }
    }
}

struct Statement<'a> {
    line: usize,
    /// Byte offset of the statement within the document.
    offset: usize,
    text: &'a str,
    /// Whether a newline follows the statement (false at EOF).
    newline: bool,
    /// Whether the line break is `\r\n`. The Go lexer consumes `\r` as the
    /// line terminator (`isNL`) and only counts a line when it later reads the
    /// `\n`, so every diagnostic that names the break differs from the `\n`
    /// spelling in its character and (for the two `errorf`/`errorPrevLine`
    /// families below) in its line number.
    crlf: bool,
}

impl Statement<'_> {
    /// The line-break character the Go lexer reports for this statement.
    fn break_char(&self) -> char {
        if self.crlf { '\r' } else { '\n' }
    }

    /// Line of an `errorf` raised after consuming the break: `\n` already
    /// advanced the lexer's line counter, `\r` has not.
    fn line_after_break(&self) -> usize {
        self.line + usize::from(!self.crlf)
    }

    /// Line of an `errorPrevLine` raised after consuming the break: it steps
    /// back one line from the counter, which only `\n` advanced, so `\r\n`
    /// reports the line before the statement (line 0 for the first line).
    fn line_before_break(&self) -> usize {
        self.line - usize::from(self.crlf)
    }
}

fn statement_at(text: &str, line: usize) -> Option<Statement<'_>> {
    let mut offset = 0;
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            let newline = raw.ends_with('\n');
            let body = raw.strip_suffix('\n').unwrap_or(raw);
            // A `\r` only pairs with the `\n` that follows it; at EOF it is a
            // lone carriage return.
            let crlf = newline && body.ends_with('\r');
            return Some(Statement {
                line,
                offset,
                text: if crlf { &body[..body.len() - 1] } else { body },
                newline,
                crlf,
            });
        }
        offset += raw.len();
    }
    None
}

/// Table path in force before `line`.
fn context(text: &str, line: usize) -> Vec<String> {
    let mut path = Vec::new();
    for raw in text.lines().take(line.saturating_sub(1)) {
        let trimmed = raw.trim_start_matches([' ', '\t']);
        if let Some(rest) = trimmed.strip_prefix('[') {
            let (_, segments) = scan_key(rest.strip_prefix('[').unwrap_or(rest));
            if !segments.is_empty() {
                path = segments;
            }
        }
    }
    path
}

fn diagnose(text: &str, offset: usize, upstream: &str) -> String {
    let line = text.as_bytes()[..offset.min(text.len())]
        .split(|byte| *byte == b'\n')
        .count();
    // A string left open on the previous line surfaces on the next one in
    // `toml_edit`, but the Go lexer reports the line that opened it.
    for candidate in [line.saturating_sub(1), line] {
        if candidate == 0 {
            continue;
        }
        let Some(statement) = statement_at(text, candidate) else {
            continue;
        };
        let ctx = context(text, candidate);
        let hint = if candidate == line || upstream.contains("multi-line") {
            upstream
        } else {
            ""
        };
        if let Some(message) = statement_message(&statement, &ctx, text, hint)
            && (candidate == line
                || message.contains("strings cannot contain newlines")
                || message.contains("expected '\"\"\"'"))
        {
            return message;
        }
    }
    // Wording of this shape was not measured against the Go lexer.
    envelope(line, None, upstream)
}

fn statement_message(
    statement: &Statement<'_>,
    ctx: &[String],
    whole: &str,
    upstream: &str,
) -> Option<String> {
    let trimmed = statement.text.trim_start_matches([' ', '\t']);
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    if upstream.contains("duplicate key") || upstream.contains("redefin") {
        return Some(duplicate(statement, ctx, trimmed));
    }
    let ctx_key = ctx.join(".");
    if let Some(header) = trimmed.strip_prefix('[') {
        return header_message(statement, &ctx_key, header);
    }
    if upstream.contains("multi-line") {
        return multiline_message(statement, ctx, trimmed);
    }
    key_message(statement, ctx, &ctx_key, trimmed, whole)
}

fn key_message(
    statement: &Statement<'_>,
    ctx: &[String],
    ctx_key: &str,
    trimmed: &str,
    whole: &str,
) -> Option<String> {
    let line = statement.line;
    let (end, segments) = scan_key(trimmed);
    let mut after = trimmed[end..].chars();
    match after.next() {
        Some('=') => {}
        None if segments.is_empty() => return None,
        None if !statement.newline => {
            return Some(envelope(
                line,
                None,
                "unexpected EOF; expected key separator '='",
            ));
        }
        other => {
            let first = trimmed.chars().next();
            if segments.is_empty() && matches!(first, Some('"' | '\'')) {
                return Some(envelope(
                    statement.line_before_break(),
                    None,
                    "strings cannot contain newlines",
                ));
            }
            let found = other.or(Some(statement.break_char()));
            let key = if segments.is_empty() {
                None
            } else {
                Some(ctx_key)
            };
            return Some(envelope(
                line,
                key.or(Some(ctx_key)),
                &format!("expected '.' or '=', but got {} instead", quote_rune(found)),
            ));
        }
    }
    if segments.is_empty() {
        return Some(envelope(
            line,
            None,
            "unexpected '=': key name appears blank",
        ));
    }
    let mut full = ctx.to_vec();
    full.extend(segments);
    let key = full.join(".");
    let value = after.as_str().trim_start_matches([' ', '\t']);
    value::value_message(statement, ctx_key, &key, value, whole)
}

fn duplicate(statement: &Statement<'_>, ctx: &[String], trimmed: &str) -> String {
    if let Some(header) = trimmed.strip_prefix('[') {
        let (_, segments) = scan_key(header.strip_prefix('[').unwrap_or(header));
        let parent = segments[..segments.len().saturating_sub(1)].join(".");
        return envelope(
            statement.line,
            Some(&parent),
            &format!("Key '{}' has already been defined.", segments.join(".")),
        );
    }
    let (_, segments) = scan_key(trimmed);
    let mut full = ctx.to_vec();
    full.extend(segments);
    let key = full.join(".");
    envelope(
        statement.line,
        Some(&key),
        &format!("Key '{key}' has already been defined."),
    )
}

fn header_message(statement: &Statement<'_>, ctx_key: &str, header: &str) -> Option<String> {
    let header = header.strip_prefix('[').unwrap_or(header);
    let (end, segments) = scan_key(header);
    let after = &header[end..];
    let line = statement.line;
    let empty = segments.is_empty() || header[..end].trim_end().ends_with('.');
    if empty && after.starts_with(']') {
        return Some(envelope(
            line,
            None,
            "unexpected end of table name (table names cannot be empty)",
        ));
    }
    match after.chars().next() {
        Some(']') => {
            let tail = after[1..].trim_start_matches([' ', '\t']);
            let tail = tail.strip_prefix(']').unwrap_or(tail);
            tail.trim_start_matches([' ', '\t']).chars().next().and_then(|c| {
                (c != '#').then(|| {
                    envelope(
                        line,
                        Some(&segments.join(".")),
                        &format!(
                            "expected a top-level item to end with a newline, comment, or EOF, but got {} instead",
                            quote_rune(Some(c))
                        ),
                    )
                })
            })
        }
        None if statement.newline => Some(envelope(
            statement.line_after_break(),
            Some(ctx_key),
            &format!(
                "expected '.' or ']' to end table name, but got {} instead",
                quote_rune(Some(statement.break_char()))
            ),
        )),
        None => Some(envelope(
            line,
            None,
            "expected '.' or ']' to end table name, but got '\\x00' instead",
        )),
        Some(c) => Some(envelope(
            line,
            Some(ctx_key),
            &format!(
                "expected '.' or ']' to end table name, but got {} instead",
                quote_rune(Some(c))
            ),
        )),
    }
}

fn multiline_message(statement: &Statement<'_>, ctx: &[String], trimmed: &str) -> Option<String> {
    let (end, segments) = scan_key(trimmed);
    let value = trimmed[end..]
        .strip_prefix('=')?
        .trim_start_matches([' ', '\t']);
    if !value.starts_with("\"\"\"") {
        // Only the basic multi-line delimiter was measured against Go.
        return None;
    }
    let mut full = ctx.to_vec();
    full.extend(segments);
    Some(envelope(
        statement.line,
        Some(&full.join(".")),
        "unexpected EOF; expected '\"\"\"'",
    ))
}

#[cfg(test)]
#[path = "setup_config_toml_tests.rs"]
mod tests;
