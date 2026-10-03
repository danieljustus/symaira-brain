//! Go flag-package argument boundaries for bounded activity reads.
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::config::format_go_quoted;
#[derive(Default)]
pub(super) struct Args {
    pub db: String,
    pub from: Vec<u8>,
    pub to: Vec<u8>,
    pub limit: i64,
    pub budget: i64,
    pub positional: Vec<String>,
}
const DEFAULTS: &str = "  -db string\n    \tdatabase path override\n  -from string\n    \trequired RFC3339 window start\n  -limit int\n    \trequired result limit (1-50)\n  -max-tokens int\n    \trequired response token budget (1-4000)\n  -profile string\n    \tprofile that explicitly exposes activity read tools\n  -to string\n    \trequired RFC3339 window end (at most 7 days)\n";

// Go checks unsigned overflow while scanning bytes, then underscore syntax,
// then signed range. Whole-value UTF-8 or early separator validation would
// change which diagnostic wins when an argument has several invalid parts.
fn parse_integer(raw: &[u8]) -> Result<i64, &'static str> {
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
#[cfg(unix)]
fn bytes(arg: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}
#[cfg(not(unix))]
fn bytes(arg: &std::ffi::OsStr) -> Vec<u8> {
    arg.to_string_lossy().as_bytes().to_vec()
}
#[cfg(unix)]
fn os(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}
#[cfg(not(unix))]
fn os(bytes: Vec<u8>) -> OsString {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
        .into()
}
// Go decodes each malformed UTF-8 byte as an individual RuneError.
fn go_string(mut bytes: &[u8]) -> String {
    let mut out = String::new();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                out.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                out.push_str(std::str::from_utf8(&bytes[..valid]).expect("valid UTF-8 prefix"));
                out.push('\u{fffd}');
                bytes = &bytes[valid + 1..];
            }
        }
    }
    out
}
pub(super) fn parse(args: &[OsString], verb: &str, stderr: &mut dyn Write) -> Result<Args, ()> {
    let normalized = crate::normalize_flags(args);
    let mut parsed = Args::default();
    let mut i = 0;
    while let Some(arg) = normalized.get(i) {
        let arg = bytes(arg);
        if arg == b"--" {
            i += 1;
            break;
        }
        if !arg.starts_with(b"-") || arg == b"-" {
            break;
        }
        let flag = arg.strip_prefix(b"--").unwrap_or(&arg[1..]);
        let (name, inline) = flag
            .iter()
            .position(|b| *b == b'=')
            .map_or((flag, None), |n| (&flag[..n], Some(&flag[n + 1..])));
        if name.is_empty() || name.starts_with(b"-") || name.starts_with(b"=") {
            let _ = stderr.write_all(b"bad flag syntax: ");
            let _ = stderr.write_all(&arg);
            let _ = stderr.write_all(b"\n");
            usage(verb, stderr);
            return Err(());
        }
        if ![
            b"db".as_slice(),
            b"from",
            b"to",
            b"limit",
            b"max-tokens",
            b"profile",
        ]
        .contains(&name)
        {
            if !matches!(name, b"h" | b"help") {
                let _ = stderr.write_all(b"flag provided but not defined: -");
                let _ = stderr.write_all(name);
                let _ = stderr.write_all(b"\n");
            }
            usage(verb, stderr);
            return Err(());
        }
        let value = inline.map(<[u8]>::to_vec).or_else(|| {
            i += 1;
            normalized.get(i).map(|v| bytes(v))
        });
        let Some(value) = value else {
            let _ = writeln!(
                stderr,
                "flag needs an argument: -{}",
                String::from_utf8_lossy(name)
            );
            usage(verb, stderr);
            return Err(());
        };
        match name {
            b"db" => parsed.db = go_string(&value),
            b"from" => parsed.from = value,
            b"to" => parsed.to = value,
            b"limit" | b"max-tokens" => {
                let number = parse_integer(&value);
                match number {
                    Ok(n) => {
                        if name == b"limit" {
                            parsed.limit = n;
                        } else {
                            parsed.budget = n;
                        }
                    }
                    Err(error) => {
                        let _ = writeln!(
                            stderr,
                            "invalid value {} for flag -{}: {error}",
                            format_go_quoted(&os(value)),
                            String::from_utf8_lossy(name)
                        );
                        usage(verb, stderr);
                        return Err(());
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    parsed.positional = normalized[i..]
        .iter()
        .map(|v| go_string(&bytes(v)))
        .collect();
    Ok(parsed)
}
fn usage(verb: &str, stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "Usage of activity {verb}:");
    let _ = write!(stderr, "{DEFAULTS}");
}
