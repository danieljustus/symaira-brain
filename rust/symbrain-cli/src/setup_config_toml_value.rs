//! Value-level wording of the Go lexer diagnostics (strings, arrays, scalars).
//!
//! Split from `setup_config_toml.rs`; the statement/key level reconstruction
//! and the shared helpers (`envelope`, `quote_rune`, `scan_key`) stay there.
use symbrain_core::config::format_go_quoted_bytes;

use super::{Statement, envelope, quote_rune, scan_key};

pub(super) fn value_message(
    statement: &Statement<'_>,
    ctx_key: &str,
    key: &str,
    value: &str,
    whole: &str,
) -> Option<String> {
    let line = statement.line;
    let with_key = |message: &str| Some(envelope(line, Some(key), message));
    if value.starts_with("\"\"\"") || value.starts_with("'''") {
        return None;
    }
    if let Some(control) = statement
        .text
        .chars()
        .find(|c| c.is_control() && *c != '\t')
    {
        return with_key(&format!(
            "TOML files cannot contain control characters: '0x{:02x}'",
            control as u32
        ));
    }
    let Some(first) = value.chars().next() else {
        return if statement.newline {
            with_key("expected value but found '\\n' instead")
        } else {
            with_key("unexpected EOF; expected value")
        };
    };
    match first {
        '=' | ']' | ',' | '#' | '}' => with_key(&format!(
            "expected value but found {} instead",
            quote_rune(Some(first))
        )),
        '"' | '\'' => string_message(statement, ctx_key, key, value, first),
        '[' => {
            let from = statement.offset + statement.text.len() - value.len();
            array_message(statement, key, &whole[from..])
        }
        '{' => {
            let inner = value[1..].trim();
            if inner.contains('}') {
                return None;
            }
            let (_, segments) = scan_key(inner);
            let full = if segments.is_empty() {
                key.to_owned()
            } else {
                format!("{key}.{}", segments.join("."))
            };
            Some(envelope(
                line,
                Some(&full),
                "expected a comma or an inline table terminator '}', but got end of file instead",
            ))
        }
        _ => word_message(statement, ctx_key, key, value),
    }
}

fn string_message(
    statement: &Statement<'_>,
    ctx_key: &str,
    key: &str,
    value: &str,
    quote: char,
) -> Option<String> {
    let line = statement.line;
    let mut chars = value[1..].char_indices();
    while let Some((index, c)) = chars.next() {
        if c == quote {
            // The string is complete; any trailing token ends the statement.
            let rest = value[index + 2..].trim_start_matches([' ', '\t']);
            return rest.chars().next().and_then(|c| {
                (c != '#').then(|| {
                    envelope(
                        line,
                        Some(ctx_key),
                        &format!(
                            "expected a top-level item to end with a newline, comment, or EOF, but got {} instead",
                            quote_rune(Some(c))
                        ),
                    )
                })
            });
        }
        if quote == '"' && c == '\\' {
            match chars.next().map(|(_, escaped)| escaped) {
                Some('b' | 't' | 'n' | 'f' | 'r' | '"' | '\\' | 'u' | 'U' | 'e' | 'x') => {}
                Some(other) => {
                    return Some(envelope(
                        line,
                        Some(key),
                        &format!("invalid escape in string '\\{other}'"),
                    ));
                }
                None => break,
            }
        }
    }
    if statement.newline {
        Some(envelope(line, Some(key), "strings cannot contain newlines"))
    } else {
        Some(envelope(
            line,
            Some(key),
            &format!(
                "unexpected EOF; expected {}",
                if quote == '"' { "'\"'" } else { "\"'\"" }
            ),
        ))
    }
}

fn array_message(statement: &Statement<'_>, key: &str, remainder: &str) -> Option<String> {
    let depth = remainder.chars().fold(0_i32, |depth, c| match c {
        '[' => depth + 1,
        ']' => depth - 1,
        _ => depth,
    });
    if depth <= 0 {
        return None;
    }
    let last = remainder.trim_end().chars().last().unwrap_or('[');
    let message = if matches!(last, ',' | '[') {
        "unexpected EOF; expected value"
    } else {
        "expected a comma (',') or array terminator (']'), but got end of file"
    };
    Some(envelope(statement.line, Some(key), message))
}

fn word_message(
    statement: &Statement<'_>,
    ctx_key: &str,
    key: &str,
    value: &str,
) -> Option<String> {
    let line = statement.line;
    let end = value
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ']' | '}' | '#' | '='))
        .unwrap_or(value.len());
    let word = &value[..end];
    let rest = value[end..].trim_start_matches([' ', '\t']);
    let quoted = format_go_quoted_bytes(word.as_bytes());
    if word.starts_with('.') {
        return Some(envelope(
            line,
            Some(key),
            "floats must start with a digit, not '.'",
        ));
    }
    if matches!(word, "true" | "false") {
        return None;
    }
    if !word.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-')) {
        return Some(envelope(
            line,
            Some(key),
            &format!("expected value but found {quoted} instead"),
        ));
    }
    let digits = word.trim_start_matches(['+', '-']);
    let all_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    let message = if digits.is_empty() && statement.newline {
        return Some(envelope(
            line + 1,
            Some(key),
            "expected a digit but got '\\n'",
        ));
    } else if all_digits && word.parse::<i64>().is_err() {
        format!("{word} is out of range for int64")
    } else if digits.len() > 1 && digits.starts_with('0') && all_digits {
        format!("Invalid integer {quoted}: cannot have leading zeroes")
    } else if word.ends_with('_') {
        format!("Invalid integer {quoted}: underscores must be surrounded by digits")
    } else if word.ends_with('.') {
        format!("Invalid float {quoted}: '.' must be followed by one or more digits")
    } else if let Some(hex) = word.strip_prefix("0x") {
        let bad = hex.char_indices().find(|(_, c)| !c.is_ascii_hexdigit())?;
        format!(
            "not a hexadecimal number: '0x{}'",
            &hex[..bad.0 + bad.1.len_utf8()]
        )
    } else if word.len() >= 10 && word.as_bytes().get(4) == Some(&b'-') {
        format!("invalid datetime: {quoted}")
    } else if all_digits && (rest.is_empty() || rest.starts_with('#')) {
        return None;
    } else {
        // `1 2` and `12abc` end the value early; the next token is reported.
        let token = if all_digits {
            rest.chars().next()
        } else {
            word.chars().find(|c| !c.is_ascii_digit())
        };
        return Some(envelope(
            line,
            Some(ctx_key),
            &format!(
                "expected a top-level item to end with a newline, comment, or EOF, but got {} instead",
                quote_rune(token)
            ),
        ));
    };
    Some(envelope(line, Some(key), &message))
}
