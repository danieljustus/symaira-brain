//! `log/slog` default-handler lines on stderr:
//! `2006/01/02 15:04:05 LEVEL message key=value …` in local time, values
//! quoted like the text handler does when they contain spaces, `=`, quotes
//! or non-printable characters.

use std::fmt::Write as _;
use std::io::Write;

fn needs_quoting(value: &str) -> bool {
    value.is_empty()
        || value
            .chars()
            .any(|c| c == ' ' || c == '=' || c == '"' || c.is_control())
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn emit(level: &str, message: &str, attrs: &[(&str, String)]) {
    let mut line = format!(
        "{} {level} {message}",
        chrono::Local::now().format("%Y/%m/%d %H:%M:%S")
    );
    for (key, value) in attrs {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        if needs_quoting(value) {
            line.push_str(&quote(value));
        } else {
            line.push_str(value);
        }
    }
    line.push('\n');
    let _ = std::io::stderr().write_all(line.as_bytes());
}

pub fn info(message: &str, attrs: &[(&str, String)]) {
    emit("INFO", message, attrs);
}

pub fn warn(message: &str, attrs: &[(&str, String)]) {
    emit("WARN", message, attrs);
}

pub fn error(message: &str, attrs: &[(&str, String)]) {
    emit("ERROR", message, attrs);
}

/// `time.Duration.String` for the request-duration attribute.
pub fn duration(elapsed: std::time::Duration) -> String {
    let nanos = elapsed.as_nanos();
    if nanos < 1_000 {
        return format!("{nanos}ns");
    }
    let scaled = |divisor: u128, unit: &str| {
        let whole = nanos / divisor;
        let fraction = nanos % divisor;
        let width = divisor.ilog10() as usize;
        let digits = format!("{fraction:0width$}");
        let digits = digits.trim_end_matches('0');
        if digits.is_empty() {
            format!("{whole}{unit}")
        } else {
            format!("{whole}.{digits}{unit}")
        }
    };
    if nanos < 1_000_000 {
        scaled(1_000, "µs")
    } else if nanos < 1_000_000_000 {
        scaled(1_000_000, "ms")
    } else {
        let seconds = elapsed.as_secs();
        if seconds < 60 {
            scaled(1_000_000_000, "s")
        } else {
            let rest = nanos - u128::from(seconds / 60 * 60) * 1_000_000_000;
            let tail = {
                let whole = rest / 1_000_000_000;
                let fraction = rest % 1_000_000_000;
                let digits = format!("{fraction:09}");
                let digits = digits.trim_end_matches('0');
                if digits.is_empty() {
                    format!("{whole}s")
                } else {
                    format!("{whole}.{digits}s")
                }
            };
            format!("{}m{tail}", seconds / 60)
        }
    }
}
