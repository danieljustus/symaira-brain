//! Validate native skills sync flags before any filesystem work.

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::config::format_go_quoted_bytes as format_go_quoted;
use symbrain_core::exit;

use super::sync_bytes as bytes;

const USAGE: &str = "Usage of skills sync:\n  -dry-run\n    \treport the plan without writing\n  -scope string\n    \tinstall scope: user or project (default \"user\")\n  -target string\n    \tlimit to one harness target\n";

pub(super) struct Flags {
    pub(super) dry_run: bool,
    pub(super) target: Option<String>,
    pub(super) scope: String,
}

pub(super) fn parse(args: &[OsString], stderr: &mut dyn Write) -> Result<Flags, u8> {
    // Go normalizes the entire vector, including separated flag values, once.
    let args = bytes::normalize(args);
    let mut dry_run = false;
    let mut target = Vec::new();
    let mut scope = b"user".to_vec();
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let arg = raw.as_slice();
        if arg == b"--" || arg == b"-" || !arg.starts_with(b"-") {
            break;
        }
        let name_start = if arg.starts_with(b"--") { 2 } else { 1 };
        let spelling = &arg[name_start..];
        if spelling.is_empty() || matches!(spelling.first(), Some(b'-' | b'=')) {
            return raw_flag_error(stderr, "bad flag syntax: ", raw);
        }
        let name_end = arg
            .iter()
            .position(|byte| *byte == b'=')
            .unwrap_or(arg.len());
        let name = &arg[name_start..name_end];
        let inline = (name_end < arg.len()).then(|| raw[name_end + 1..].to_vec());
        match name {
            b"h" | b"help" => return flag_error(stderr, None),
            b"dry-run" => {
                dry_run = match inline.as_deref().map(std::str::from_utf8) {
                    None | Some(Ok("1" | "t" | "T" | "true" | "TRUE" | "True")) => true,
                    Some(Ok("0" | "f" | "F" | "false" | "FALSE" | "False")) => false,
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
            b"target" | b"scope" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return flag_error(
                            stderr,
                            Some(format!(
                                "flag needs an argument: -{}",
                                if name == b"target" { "target" } else { "scope" },
                            )),
                        );
                    };
                    value.clone()
                };
                if name == b"target" {
                    target = value;
                } else {
                    scope = value;
                }
            }
            _ => {
                return raw_flag_error(
                    stderr,
                    "flag provided but not defined: -",
                    &raw[name_start..name_end],
                );
            }
        }
        index += 1;
    }
    validate(dry_run, &target, &scope, stderr)
}

fn validate(
    dry_run: bool,
    raw_target: &[u8],
    raw_scope: &[u8],
    stderr: &mut dyn Write,
) -> Result<Flags, u8> {
    let raw_target = bytes::trim_target(raw_target);
    let target = std::str::from_utf8(raw_target).ok();
    let known = symbrain_skills::default_targets();
    if !target.is_some_and(|value| value.is_empty() || known.iter().any(|name| name == value)) {
        let quote = target.map_or_else(
            || format_go_quoted(raw_target),
            |value| format_go_quoted(value.as_bytes()),
        );
        let _ = writeln!(
            stderr,
            "symbrain skills sync: unknown target {quote} (known: {})",
            known.join(", ")
        );
        return Err(exit::USAGE);
    }
    let scope = match std::str::from_utf8(raw_scope).ok().map(str::trim) {
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

fn raw_flag_error(stderr: &mut dyn Write, prefix: &str, operand: &[u8]) -> Result<Flags, u8> {
    let _ = stderr.write_all(prefix.as_bytes());
    // Go emits unquoted operands: Unix raw argv or Windows lossless WTF-8.
    let _ = stderr.write_all(operand);
    let _ = stderr.write_all(b"\n");
    flag_error(stderr, None)
}

fn flag_error(stderr: &mut dyn Write, error: Option<String>) -> Result<Flags, u8> {
    if let Some(error) = error {
        let _ = writeln!(stderr, "{error}");
    }
    let _ = write!(stderr, "{USAGE}");
    Err(exit::USAGE)
}
