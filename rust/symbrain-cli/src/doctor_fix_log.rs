//! Go's default slog text format, with a real local-time timestamp per event.
use std::io::Write;
use symbrain_core::config::format_go_quoted_bytes;

pub(super) fn event(
    stderr: &mut dyn Write,
    level: &str,
    message: &str,
    attributes: &[(&str, &str)],
) {
    let attributes: Vec<_> = attributes
        .iter()
        .map(|(key, value)| (*key, value.as_bytes()))
        .collect();
    event_bytes(stderr, level, message, &attributes);
}

pub(super) fn event_bytes(
    stderr: &mut dyn Write,
    level: &str,
    message: &str,
    attributes: &[(&str, &[u8])],
) {
    let _ = write!(
        stderr,
        "{} {level} {message}",
        chrono::Local::now().format("%Y/%m/%d %H:%M:%S")
    );
    for (key, value) in attributes {
        // slog permits bare backslashes. All other strconv.Quote escapes,
        // whitespace, '=' and empty strings require a quoted attribute.
        let check: Vec<_> = value
            .iter()
            .map(|byte| if *byte == b'\\' { b'x' } else { *byte })
            .collect();
        let quoted = format_go_quoted_bytes(&check);
        let needs_quote = check.is_empty()
            || check.contains(&b'=')
            || check.windows(3).any(|bytes| bytes == "\u{fffd}".as_bytes())
            || std::str::from_utf8(&check).is_ok_and(|text| text.chars().any(char::is_whitespace))
            || quoted.as_bytes()[1..quoted.len() - 1] != check;
        let _ = write!(stderr, " {key}=");
        if needs_quote {
            let _ = stderr.write_all(format_go_quoted_bytes(value).as_bytes());
        } else {
            let _ = stderr.write_all(value);
        }
    }
    let _ = writeln!(stderr);
}
