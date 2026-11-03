//! The default `log/slog` text lines the shipped write pipeline prints.
//!
//! Before any handler is installed `slog` forwards to the standard `log`
//! package: `2006/01/02 15:04:05 LEVEL message key=value ...` in local time.
//! Values are quoted exactly when the text handler would quote them.

use std::io::Write;

/// A destination for log lines; the CLI passes its stderr.
pub struct LogSink<'a>(pub &'a mut dyn Write);

impl LogSink<'_> {
    /// `slog.Warn`.
    pub(crate) fn warn(&mut self, message: &str, attrs: &[(&str, &str)]) {
        self.line("WARN", message, attrs);
    }

    /// `slog.Info`.
    pub(crate) fn info(&mut self, message: &str, attrs: &[(&str, &str)]) {
        self.line("INFO", message, attrs);
    }

    fn line(&mut self, level: &str, message: &str, attrs: &[(&str, &str)]) {
        let mut text = format!(
            "{} {level} {message}",
            chrono::Local::now().format("%Y/%m/%d %H:%M:%S")
        );
        for (key, value) in attrs {
            text.push(' ');
            text.push_str(key);
            text.push('=');
            if needs_quoting(value) {
                text.push_str(&quote(value));
            } else {
                text.push_str(value);
            }
        }
        text.push('\n');
        let _ = self.0.write_all(text.as_bytes());
    }
}

/// `slog`'s `needsQuoting`.
fn needs_quoting(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    value.chars().any(|character| {
        if character.is_ascii() {
            // Everything the JSON "safe set" refuses, plus space and '=';
            // a lone backslash is not quoted by the handler.
            character != '\\'
                && (character == ' '
                    || character == '='
                    || character == '"'
                    || character.is_ascii_control()
                    || character == '\u{7f}')
        } else {
            character == '\u{fffd}' || character.is_whitespace() || character.is_control()
        }
    })
}

/// `strconv.Quote` for the characters the pipeline can print.
pub(crate) fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if u32::from(c) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", u32::from(c)));
            }
            c if c.is_control() => {
                let code = u32::from(c);
                if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `%q` of a Go string holding arbitrary bytes: invalid bytes are `\xNN`.
pub(crate) fn quote_bytes(value: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in value.utf8_chunks() {
        let quoted = quote(chunk.valid());
        out.push_str(&quoted[1..quoted.len() - 1]);
        for byte in chunk.invalid() {
            out.push_str(&format!("\\x{byte:02x}"));
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_quoted_like_the_text_handler() {
        let mut buffer = Vec::new();
        LogSink(&mut buffer).warn("m", &[("a", "plain"), ("b", "two words"), ("c", "")]);
        let text = String::from_utf8(buffer).unwrap();
        assert!(
            text.ends_with(" WARN m a=plain b=\"two words\" c=\"\"\n"),
            "{text}"
        );
    }
}
