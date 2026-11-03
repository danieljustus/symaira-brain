//! Go `flag` package model for every `symbrain skills` subcommand.
//!
//! Each Go subcommand builds `skillsFlagSet` (`-target`, `-scope`) plus its
//! own flags (`log`: `-skill`, `-limit`/`-l`; `sync`: `-dry-run`) with
//! `flag.ContinueOnError`, parses `normalizeFlags(args)` and, on failure, has
//! the flag set print the diagnostic and its usage to stderr before the
//! command exits with `ExitNoInput`. Everything after the first positional
//! argument or `--` is ignored. Values stay raw bytes until a command
//! validates them, because Go compares and quotes the exact argv bytes.

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::config::format_go_quoted_bytes as quote;
use symbrain_core::exit;

#[path = "../skills_sync_bytes.rs"]
pub(super) mod bytes;

/// The flag sets of the six subcommands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Verb {
    List,
    Status,
    Targets,
    Log,
    Sync,
    Doctor,
}

impl Verb {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Status => "status",
            Self::Targets => "targets",
            Self::Log => "log",
            Self::Sync => "sync",
            Self::Doctor => "doctor",
        }
    }

    /// `flag.FlagSet.PrintDefaults`: flags sorted by name.
    fn usage(self) -> String {
        let mut text = format!("Usage of skills {}:\n", self.name());
        if self == Self::Sync {
            text.push_str("  -dry-run\n    \treport the plan without writing\n");
        }
        if self == Self::Log {
            text.push_str(
                "  -l int\n    \tmaximum records to return, newest first\n  -limit int\n    \tmaximum records to return, newest first\n",
            );
        }
        text.push_str("  -scope string\n    \tinstall scope: user or project (default \"user\")\n");
        if self == Self::Log {
            text.push_str("  -skill string\n    \tlimit to one skill name\n");
        }
        text.push_str("  -target string\n    \tlimit to one harness target\n");
        text
    }

    const fn has_skill_and_limit(self) -> bool {
        matches!(self, Self::Log)
    }

    const fn has_dry_run(self) -> bool {
        matches!(self, Self::Sync)
    }
}

/// Parsed values, raw bytes where Go keeps a string.
#[derive(Debug)]
pub(super) struct Flags {
    pub(super) target: Vec<u8>,
    pub(super) scope: Vec<u8>,
    pub(super) skill: Vec<u8>,
    pub(super) limit: i64,
    pub(super) dry_run: bool,
}

/// Parses `args` like `fs.Parse(normalizeFlags(args))`.
///
/// # Errors
///
/// The exit code after the diagnostic and usage were written to `stderr`.
pub(super) fn parse(verb: Verb, args: &[OsString], stderr: &mut dyn Write) -> Result<Flags, u8> {
    let args = bytes::normalize(args);
    let mut flags = Flags {
        target: Vec::new(),
        scope: b"user".to_vec(),
        skill: Vec::new(),
        limit: 0,
        dry_run: false,
    };
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let arg = raw.as_slice();
        if arg == b"--" || arg == b"-" || !arg.starts_with(b"-") {
            break;
        }
        let name_start = if arg.starts_with(b"--") { 2 } else { 1 };
        if arg.len() == name_start || matches!(arg.get(name_start), Some(b'-' | b'=')) {
            return fail_raw(verb, stderr, "bad flag syntax: ", raw);
        }
        let name_end = arg
            .iter()
            .position(|byte| *byte == b'=')
            .unwrap_or(arg.len());
        let name = &arg[name_start..name_end];
        let inline = (name_end < arg.len()).then(|| arg[name_end + 1..].to_vec());
        let defined = matches!(name, b"target" | b"scope")
            || (verb.has_skill_and_limit() && matches!(name, b"skill" | b"limit" | b"l"))
            || (verb.has_dry_run() && name == b"dry-run");
        if !defined {
            if matches!(name, b"h" | b"help") {
                return fail(verb, stderr, None);
            }
            return fail_raw(verb, stderr, "flag provided but not defined: -", name);
        }
        if name == b"dry-run" {
            flags.dry_run = match inline.as_deref() {
                None | Some(b"1" | b"t" | b"T" | b"true" | b"TRUE" | b"True") => true,
                Some(b"0" | b"f" | b"F" | b"false" | b"FALSE" | b"False") => false,
                Some(other) => {
                    let message = format!(
                        "invalid boolean value {} for -dry-run: parse error",
                        quote(other)
                    );
                    return fail(verb, stderr, Some(message.as_bytes()));
                }
            };
            index += 1;
            continue;
        }
        let value = if let Some(value) = inline {
            value
        } else {
            index += 1;
            let Some(value) = args.get(index) else {
                let mut message = b"flag needs an argument: -".to_vec();
                message.extend_from_slice(name);
                return fail(verb, stderr, Some(&message));
            };
            value.clone()
        };
        match name {
            b"target" => flags.target = value,
            b"scope" => flags.scope = value,
            b"skill" => flags.skill = value,
            _ => match parse_int(&value) {
                Ok(number) => flags.limit = number,
                Err(reason) => {
                    let mut message =
                        format!("invalid value {} for flag -", quote(&value)).into_bytes();
                    message.extend_from_slice(name);
                    message.extend_from_slice(format!(": {reason}").as_bytes());
                    return fail(verb, stderr, Some(&message));
                }
            },
        }
        index += 1;
    }
    Ok(flags)
}

fn fail(verb: Verb, stderr: &mut dyn Write, message: Option<&[u8]>) -> Result<Flags, u8> {
    if let Some(message) = message {
        let _ = stderr.write_all(message);
        let _ = stderr.write_all(b"\n");
    }
    let _ = stderr.write_all(verb.usage().as_bytes());
    Err(exit::USAGE)
}

fn fail_raw(verb: Verb, stderr: &mut dyn Write, prefix: &str, operand: &[u8]) -> Result<Flags, u8> {
    // Go emits unquoted operands: raw argv bytes on Unix, WTF-8 on Windows.
    let mut message = prefix.as_bytes().to_vec();
    message.extend_from_slice(operand);
    fail(verb, stderr, Some(&message))
}

/// `strconv.underscoreOK`: underscores only between digits or after a base
/// prefix.
fn underscore_ok(text: &[u8]) -> bool {
    let mut saw = b'^';
    let mut index = 0;
    let mut hex = false;
    if text.len() >= 2
        && text[0] == b'0'
        && matches!(text[1].to_ascii_lowercase(), b'b' | b'o' | b'x')
    {
        index = 2;
        saw = b'0';
        hex = text[1].eq_ignore_ascii_case(&b'x');
    }
    for &byte in &text[index..] {
        if byte.is_ascii_digit() || (hex && matches!(byte.to_ascii_lowercase(), b'a'..=b'f')) {
            saw = b'0';
        } else if byte == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else if saw == b'_' {
            return false;
        } else {
            saw = b'!';
        }
    }
    saw != b'_'
}

/// `strconv.ParseInt(value, 0, 64)` with Go's error classes, digit by digit
/// so that an overflow before an invalid byte stays a range error.
fn parse_int(raw: &[u8]) -> Result<i64, &'static str> {
    let (negative, text) = match raw.first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    if text.is_empty() {
        return Err("parse error");
    }
    let (radix, digits) = if text[0] == b'0' {
        match text.get(1).map(u8::to_ascii_lowercase) {
            Some(b'b') if text.len() >= 3 => (2_u64, &text[2..]),
            Some(b'o') if text.len() >= 3 => (8, &text[2..]),
            Some(b'x') if text.len() >= 3 => (16, &text[2..]),
            _ => (8, &text[1..]),
        }
    } else {
        (10, text)
    };
    let mut value = 0_u64;
    let mut underscores = false;
    for &byte in digits {
        let digit = match byte {
            b'_' => {
                underscores = true;
                continue;
            }
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'z' => byte - b'a' + 10,
            b'A'..=b'Z' => byte - b'A' + 10,
            _ => return Err("parse error"),
        };
        if u64::from(digit) >= radix {
            return Err("parse error");
        }
        value = value
            .checked_mul(radix)
            .and_then(|n| n.checked_add(u64::from(digit)))
            .ok_or("value out of range")?;
    }
    if underscores && !underscore_ok(text) {
        return Err("parse error");
    }
    let limit = if negative {
        1_u64 << 63
    } else {
        i64::MAX.cast_unsigned()
    };
    if value > limit {
        return Err("value out of range");
    }
    Ok(if negative {
        value.wrapping_neg().cast_signed()
    } else {
        value.cast_signed()
    })
}

#[cfg(test)]
mod tests {
    use super::parse_int;

    #[test]
    fn parse_int_matches_strconv_base_zero() {
        for (text, expected) in [
            ("0", Ok(0)),
            ("7", Ok(7)),
            ("-3", Ok(-3)),
            ("+12", Ok(12)),
            ("0x1F", Ok(31)),
            ("0b101", Ok(5)),
            ("0o17", Ok(15)),
            ("017", Ok(15)),
            ("1_000", Ok(1000)),
            ("0x_1f", Ok(31)),
            ("0x1__f", Err("parse error")),
            ("0x1f_", Err("parse error")),
            ("", Err("parse error")),
            ("0x", Err("parse error")),
            ("09", Err("parse error")),
            ("9223372036854775807", Ok(i64::MAX)),
            ("9223372036854775808", Err("value out of range")),
            ("-9223372036854775808", Ok(i64::MIN)),
            ("-9223372036854775809", Err("value out of range")),
            ("99999999999999999999999", Err("value out of range")),
        ] {
            assert_eq!(parse_int(text.as_bytes()), expected, "{text}");
        }
    }
}
