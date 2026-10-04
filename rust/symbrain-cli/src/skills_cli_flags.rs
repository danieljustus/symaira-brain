//! The remaining skills flag sets retain Go's raw argv and positional stop.
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::{config::format_go_quoted_bytes as quote, exit};
#[path = "skills_sync_bytes.rs"]
mod bytes;

#[derive(Default)]
pub(super) struct Flags {
    pub(super) target: Option<String>,
    pub(super) skill: Option<String>,
    pub(super) limit: i64,
    raw_target: Vec<u8>,
    raw_skill: Vec<u8>,
    raw_scope: Vec<u8>,
}

impl Flags {
    pub(super) fn invalid_filter(&self) -> bool {
        std::str::from_utf8(bytes::trim_target(&self.raw_target)).is_err()
            || std::str::from_utf8(bytes::trim_target(&self.raw_skill)).is_err()
    }

    pub(super) fn scope(&self, verb: &str, stderr: &mut dyn Write) -> Result<String, u8> {
        match bytes::trim_target(&self.raw_scope) {
            b"" | b"user" => Ok("user".into()),
            b"project" => Ok("project".into()),
            _ => {
                let _ = writeln!(
                    stderr,
                    "symbrain skills {verb}: unknown scope {} (known: user, project)",
                    quote(&self.raw_scope)
                );
                Err(exit::USAGE)
            }
        }
    }
    pub(super) fn target_scope(
        &self,
        verb: &str,
        stderr: &mut dyn Write,
    ) -> Result<(Option<String>, String), u8> {
        let target = bytes::trim_target(&self.raw_target);
        let known = symbrain_skills::default_targets();
        if !target.is_empty() && !known.iter().any(|name| name.as_bytes() == target) {
            let _ = writeln!(
                stderr,
                "symbrain skills {verb}: unknown target {} (known: {})",
                quote(target),
                known.join(", ")
            );
            return Err(exit::USAGE);
        }
        Ok((
            (!target.is_empty()).then(|| String::from_utf8_lossy(target).into_owned()),
            self.scope(verb, stderr)?,
        ))
    }
}

fn usage(verb: &str, stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "Usage of skills {verb}:");
    if verb == "log" {
        let _ = write!(
            stderr,
            "  -l int\n    \tmaximum records to return, newest first\n  -limit int\n    \tmaximum records to return, newest first\n"
        );
    }
    let _ = write!(
        stderr,
        "  -scope string\n    \tinstall scope: user or project (default \"user\")\n"
    );
    if verb == "log" {
        let _ = write!(stderr, "  -skill string\n    \tlimit to one skill name\n");
    }
    let _ = write!(
        stderr,
        "  -target string\n    \tlimit to one harness target\n"
    );
}

pub(super) fn parse(verb: &str, args: &[OsString], stderr: &mut dyn Write) -> Result<Flags, u8> {
    let args = bytes::normalize(args);
    let mut flags = Flags {
        raw_scope: b"user".to_vec(),
        ..Flags::default()
    };
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        if raw == b"--" || raw == b"-" || !raw.starts_with(b"-") {
            break;
        }
        let offset = if raw.starts_with(b"--") { 2 } else { 1 };
        if raw.len() == offset || matches!(raw.get(offset), Some(b'-' | b'=')) {
            return error(verb, stderr, b"bad flag syntax: ", raw);
        }
        let end = raw
            .iter()
            .position(|byte| *byte == b'=')
            .unwrap_or(raw.len());
        let name = &raw[offset..end];
        if matches!(name, b"h" | b"help") {
            usage(verb, stderr);
            return Err(exit::USAGE);
        }
        if !matches!(name, b"target" | b"scope")
            && !(verb == "log" && matches!(name, b"skill" | b"limit" | b"l"))
        {
            return error(verb, stderr, b"flag provided but not defined: -", name);
        }
        let value = if end < raw.len() {
            raw[end + 1..].clone()
        } else {
            index += 1;
            let Some(value) = args.get(index) else {
                return error(verb, stderr, b"flag needs an argument: -", name);
            };
            value.clone()
        };
        match name {
            b"scope" => flags.raw_scope = value,
            b"target" => {
                flags.target = Some(String::from_utf8_lossy(&value).into_owned());
                flags.raw_target = value;
            }
            b"skill" => {
                flags.skill = Some(String::from_utf8_lossy(&value).into_owned());
                flags.raw_skill = value;
            }
            _ => {
                let parsed = parse_int(&value);
                match parsed {
                    Ok(number) => flags.limit = number,
                    Err(reason) => {
                        let _ = write!(stderr, "invalid value {} for flag -", quote(&value));
                        let _ = stderr.write_all(name);
                        let _ = writeln!(stderr, ": {reason}");
                        usage(verb, stderr);
                        return Err(exit::USAGE);
                    }
                }
            }
        }
        index += 1;
    }
    Ok(flags)
}

fn parse_int(raw: &[u8]) -> Result<i64, &'static str> {
    let (negative, text) = match raw.first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let (radix, digits, prefix) = if let Some(d) = text
        .strip_prefix(b"0x")
        .or_else(|| text.strip_prefix(b"0X"))
    {
        (16, d, true)
    } else if let Some(d) = text
        .strip_prefix(b"0b")
        .or_else(|| text.strip_prefix(b"0B"))
    {
        (2, d, true)
    } else if let Some(d) = text
        .strip_prefix(b"0o")
        .or_else(|| text.strip_prefix(b"0O"))
    {
        (8, d, true)
    } else if text.starts_with(b"0") && text.len() > 1 {
        (8, &text[1..], true)
    } else {
        (10, text, false)
    };
    if digits.is_empty() {
        return Err("parse error");
    }
    let mut value = 0_u64;
    for &byte in digits {
        if byte == b'_' {
            continue;
        }
        let digit = match byte {
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
    let mut last_digit = prefix;
    for &byte in digits {
        if byte == b'_' && !last_digit {
            return Err("parse error");
        }
        last_digit = byte != b'_';
    }
    if !last_digit {
        return Err("parse error");
    }
    let maximum = if negative {
        1_u64 << 63
    } else {
        i64::MAX.cast_unsigned()
    };
    if value > maximum {
        return Err("value out of range");
    }
    Ok(if negative {
        value.wrapping_neg().cast_signed()
    } else {
        value.cast_signed()
    })
}
fn error(verb: &str, stderr: &mut dyn Write, prefix: &[u8], raw: &[u8]) -> Result<Flags, u8> {
    let _ = stderr.write_all(prefix);
    let _ = stderr.write_all(raw);
    let _ = stderr.write_all(b"\n");
    usage(verb, stderr);
    Err(exit::USAGE)
}

pub(super) fn quote_argument(value: &std::ffi::OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        quote(value.as_bytes())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut raw = Vec::new();
        for ch in char::decode_utf16(value.encode_wide()) {
            match ch {
                Ok(ch) => raw.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes()),
                Err(error) => {
                    let unit = error.unpaired_surrogate();
                    raw.extend_from_slice(&[
                        0xe0 | (unit >> 12) as u8,
                        0x80 | ((unit >> 6) & 63) as u8,
                        0x80 | (unit & 63) as u8,
                    ]);
                }
            }
        }
        quote(&raw)
    }
    #[cfg(not(any(unix, windows)))]
    quote(value.to_string_lossy().as_bytes())
}
