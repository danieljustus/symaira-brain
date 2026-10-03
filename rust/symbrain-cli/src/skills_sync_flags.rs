//! Validate native skills sync flags before any filesystem work.

use std::ffi::{OsStr, OsString};
use std::io::Write;

use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;

const USAGE: &str = "Usage of skills sync:\n  -dry-run\n    \treport the plan without writing\n  -scope string\n    \tinstall scope: user or project (default \"user\")\n  -target string\n    \tlimit to one harness target\n";

pub(super) struct Flags {
    pub(super) dry_run: bool,
    pub(super) target: Option<String>,
    pub(super) scope: String,
}

pub(super) fn parse(args: &[OsString], stderr: &mut dyn Write) -> Result<Flags, u8> {
    // Go normalizes the entire vector, including separated flag values, once.
    let args = crate::normalize_flags(args);
    let mut dry_run = false;
    let mut target = OsString::new();
    let mut scope = OsString::from("user");
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let arg = raw.to_string_lossy();
        if arg == "--" || arg == "-" || !arg.starts_with('-') {
            break;
        }
        let name = arg
            .strip_prefix("--")
            .or_else(|| arg.strip_prefix('-'))
            .unwrap_or_default();
        if name.is_empty() || name.starts_with(['-', '=']) {
            return flag_error(stderr, Some(format!("bad flag syntax: {arg}")));
        }
        let (name, inline) = name.split_once('=').map_or((name, None), |(name, _)| {
            // Known flag names are ASCII, so this offset is byte/unit invariant.
            let offset = raw
                .as_encoded_bytes()
                .iter()
                .position(|byte| *byte == b'=')
                .expect("ASCII equals is preserved in flag spelling");
            (name, Some(suffix(raw, offset + 1)))
        });
        match name {
            "h" | "help" => return flag_error(stderr, None),
            "dry-run" => {
                dry_run = match inline.as_deref().map(OsStr::to_str) {
                    None | Some(Some("1" | "t" | "T" | "true" | "TRUE" | "True")) => true,
                    Some(Some("0" | "f" | "F" | "false" | "FALSE" | "False")) => false,
                    _ => {
                        return flag_error(
                            stderr,
                            Some(format!(
                                "invalid boolean value {} for -dry-run: parse error",
                                format_go_quoted(inline.as_deref().unwrap()),
                            )),
                        );
                    }
                };
            }
            "target" | "scope" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return flag_error(
                            stderr,
                            Some(format!("flag needs an argument: -{name}")),
                        );
                    };
                    value.clone()
                };
                if name == "target" {
                    target = value;
                } else {
                    scope = value;
                }
            }
            _ => {
                return flag_error(
                    stderr,
                    Some(format!("flag provided but not defined: -{name}")),
                );
            }
        }
        index += 1;
    }
    validate(dry_run, &target, &scope, stderr)
}

fn validate(
    dry_run: bool,
    raw_target: &OsStr,
    raw_scope: &OsStr,
    stderr: &mut dyn Write,
) -> Result<Flags, u8> {
    let raw_target = trim_target(raw_target);
    let target = raw_target.to_str();
    let known = symbrain_skills::default_targets();
    if !target.is_some_and(|value| value.is_empty() || known.iter().any(|name| name == value)) {
        let quote = target.map_or_else(
            || format_go_quoted(raw_target),
            |value| format_go_quoted(OsStr::new(value)),
        );
        let _ = writeln!(
            stderr,
            "symbrain skills sync: unknown target {quote} (known: {})",
            known.join(", ")
        );
        return Err(exit::USAGE);
    }
    let scope = match raw_scope.to_str().map(str::trim) {
        Some("" | "user") => "user",
        Some("project") => "project",
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills sync: unknown scope {} (known: user, project)",
                format_go_quoted(raw_scope)
            );
            return Err(exit::USAGE);
        }
    };
    Ok(Flags {
        dry_run,
        target: target.filter(|value| !value.is_empty()).map(str::to_owned),
        scope: scope.to_owned(),
    })
}

fn trim_target(value: &OsStr) -> &OsStr {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        // Go TrimSpace stops at invalid UTF-8. Decode only boundary runes so
        // invalid interior bytes survive unchanged in the diagnostic.
        let whitespace = |bytes: &[u8]| {
            std::str::from_utf8(bytes).is_ok_and(|text| {
                let mut chars = text.chars();
                chars.next().is_some_and(char::is_whitespace) && chars.next().is_none()
            })
        };
        let mut bytes = value.as_bytes();
        while let Some(width) = (1..=bytes.len().min(4)).find(|&n| whitespace(&bytes[..n])) {
            bytes = &bytes[width..];
        }
        while let Some(width) =
            (1..=bytes.len().min(4)).find(|&n| whitespace(&bytes[bytes.len() - n..]))
        {
            bytes = &bytes[..bytes.len() - width];
        }
        OsStr::from_bytes(bytes)
    }
    #[cfg(not(unix))]
    {
        value.to_str().map_or(value, |text| OsStr::new(text.trim()))
    }
}

fn suffix(arg: &OsStr, offset: usize) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        OsString::from_vec(arg.as_bytes()[offset..].to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(&arg.to_string_lossy()[offset..])
    }
}

fn flag_error(stderr: &mut dyn Write, error: Option<String>) -> Result<Flags, u8> {
    if let Some(error) = error {
        let _ = writeln!(stderr, "{error}");
    }
    let _ = write!(stderr, "{USAGE}");
    Err(exit::USAGE)
}
