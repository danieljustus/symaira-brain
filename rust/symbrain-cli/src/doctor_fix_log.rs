//! Go's default slog text format, with a real local-time timestamp per event.
use std::io::Write;
use symbrain_core::config::format_go_quoted;

pub(super) fn event(
    stderr: &mut dyn Write,
    level: &str,
    message: &str,
    attributes: &[(&str, &str)],
) {
    let _ = write!(
        stderr,
        "{} {level} {message}",
        chrono::Local::now().format("%Y/%m/%d %H:%M:%S")
    );
    for (key, value) in attributes {
        // slog permits bare backslashes. All other strconv.Quote escapes,
        // whitespace, '=' and empty strings require a quoted attribute.
        let check = value.replace('\\', "x");
        let quoted = format_go_quoted(check.as_ref());
        let value = if check.is_empty()
            || check.contains('=')
            || check.contains('\u{fffd}')
            || check.chars().any(char::is_whitespace)
            || quoted[1..quoted.len() - 1] != check
        {
            format_go_quoted((*value).as_ref())
        } else {
            (*value).to_owned()
        };
        let _ = write!(stderr, " {key}={value}");
    }
    let _ = writeln!(stderr);
}
