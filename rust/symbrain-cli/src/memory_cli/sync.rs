//! `memory sync`: the argument checks that precede the shipped
//! implementation's database and network work (flag grammar, positional
//! refusal, the missing remote, the relay passphrase and the remote URL
//! guard), then the native exchange for the proven loopback subset
//! (`sync_native`). Every other valid invocation (or a remote spelling
//! outside the proven URL subset) returns `None` and stays on Go.

use super::help::{MEMORY_SYNC_REMOTE_REQUIRED, MEMORY_SYNC_USAGE};
use super::{OsString, flags, sync_native};
use std::io::Write;
use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;

const PASSPHRASE_ENV: &str = "SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE";

enum Outcome {
    /// The invocation fails before any database or network work.
    Refused(u8),
    /// A valid invocation (or an unproven URL spelling).
    Proceed(flags::Args),
}

/// Runs `memory sync`; `None` leaves the invocation to the shipped
/// implementation (nothing has been written to the streams then).
pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    match preflight(args, stderr) {
        Outcome::Refused(code) => Some(code),
        Outcome::Proceed(parsed) => sync_native::run(&parsed, stdout, stderr, format),
    }
}

fn preflight(args: &[OsString], stderr: &mut dyn Write) -> Outcome {
    let Ok(parsed) = flags::parse(args, "sync", stderr) else {
        return Outcome::Refused(exit::USAGE);
    };
    if let Some(first) = parsed.positional.first() {
        let _ = writeln!(
            stderr,
            "symbrain memory sync: unexpected argument {}",
            format_go_quoted(first)
        );
        return Outcome::Refused(exit::USAGE);
    }
    let remote = parsed.bytes_of("remote");
    if remote.is_empty() {
        let _ = write!(stderr, "{MEMORY_SYNC_REMOTE_REQUIRED}{MEMORY_SYNC_USAGE}");
        return Outcome::Refused(exit::USAGE);
    }
    if parsed.flag("encrypted-relay")
        && parsed.bytes_of("relay-passphrase").is_empty()
        && std::env::var_os(PASSPHRASE_ENV).is_none_or(|value| value.is_empty())
    {
        let _ = writeln!(
            stderr,
            "symbrain memory sync: --encrypted-relay requires --relay-passphrase (or ${PASSPHRASE_ENV})"
        );
        return Outcome::Refused(exit::USAGE);
    }
    match validate_remote(remote, parsed.flag("allow-insecure-http")) {
        Verdict::Valid | Verdict::Unproven => Outcome::Proceed(parsed),
        Verdict::Rejected(message) => {
            let _ = writeln!(stderr, "symbrain memory sync: {message}");
            Outcome::Refused(exit::USAGE)
        }
    }
}

enum Verdict {
    Valid,
    Rejected(String),
    /// Outside the subset whose `net/url` behaviour is ported.
    Unproven,
}

/// `syncclient.ValidateRemoteURL` for the plain ASCII subset of `net/url`
/// (no escapes, userinfo, IPv6 literals, quotes or control bytes); every
/// other spelling is `Unproven` and keeps the shipped implementation.
fn validate_remote(raw: &[u8], allow_insecure: bool) -> Verdict {
    if !raw.iter().all(|&byte| plain_url_byte(byte)) {
        return Verdict::Unproven;
    }
    let raw = std::str::from_utf8(raw).expect("ASCII subset");
    let parse_error = |url: &str, reason: &str| {
        Verdict::Rejected(format!("parse remote URL: parse {url:?}: {reason}"))
    };
    // url.Parse cuts the fragment first and reports errors against the rest.
    let without_fragment = raw.split_once('#').map_or(raw, |(head, _)| head);
    let (scheme, rest) = match split_scheme(without_fragment) {
        Err(()) => return parse_error(without_fragment, "missing protocol scheme"),
        Ok(split) => split,
    };
    let rest = rest.split_once('?').map_or(rest, |(head, _)| head);
    let lower = scheme.to_ascii_lowercase();
    if !rest.starts_with('/') {
        if lower.is_empty() {
            let segment = rest.split('/').next().unwrap_or("");
            if segment.contains(':') {
                return parse_error(
                    without_fragment,
                    "first path segment in URL cannot contain colon",
                );
            }
            return Verdict::Rejected(format!("parse remote URL: missing scheme in {raw:?}"));
        }
        // An opaque URL has no host.
        return judge(&lower, "", raw, allow_insecure);
    }
    let mut host = "";
    if rest.starts_with("//") && (!lower.is_empty() || !rest.starts_with("///")) {
        let authority = &rest[2..];
        host = authority.split('/').next().unwrap_or("");
        // http(s) hosts are strict: the port starts at the first colon, so a
        // second colon makes the port invalid; other schemes use the last one.
        let split = if matches!(lower.as_str(), "http" | "https") {
            host.find(':')
        } else {
            host.rfind(':')
        };
        if let Some(colon) = split
            && !valid_optional_port(&host[colon..])
        {
            return parse_error(
                without_fragment,
                &format!("invalid port {:?} after host", &host[colon..]),
            );
        }
    }
    if lower.is_empty() {
        return Verdict::Rejected(format!("parse remote URL: missing scheme in {raw:?}"));
    }
    judge(&lower, host, raw, allow_insecure)
}

fn judge(scheme: &str, host: &str, raw: &str, allow_insecure: bool) -> Verdict {
    match scheme {
        "https" => Verdict::Valid,
        "http" => {
            let hostname = hostname(host);
            if hostname.is_empty() {
                return Verdict::Rejected(format!(
                    "remote memory sync requires an https URL: http URL {raw:?} has no host"
                ));
            }
            if hostname.eq_ignore_ascii_case("localhost")
                || is_loopback_v4(hostname)
                || allow_insecure
            {
                return Verdict::Valid;
            }
            Verdict::Rejected(format!(
                "remote memory sync requires an https URL: {raw} (pass --allow-insecure-http to override)"
            ))
        }
        other => Verdict::Rejected(format!(
            "remote URL scheme {other:?} is not supported (use https, or http for loopback)"
        )),
    }
}

/// Bytes whose handling by `net/url` is ported: printable ASCII minus the
/// escape, userinfo, IPv6-literal, quoting and backslash characters.
fn plain_url_byte(byte: u8) -> bool {
    byte.is_ascii_graphic()
        && !matches!(
            byte,
            b'%' | b'@'
                | b'['
                | b']'
                | b'\\'
                | b'"'
                | b'<'
                | b'>'
                | b'^'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
        )
}

/// Go `getScheme`: `Err` is the empty-scheme-before-colon error.
fn split_scheme(raw: &str) -> Result<(&str, &str), ()> {
    for (index, byte) in raw.bytes().enumerate() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if index == 0 {
                    return Ok(("", raw));
                }
            }
            b':' => {
                if index == 0 {
                    return Err(());
                }
                return Ok((&raw[..index], &raw[index + 1..]));
            }
            _ => return Ok(("", raw)),
        }
    }
    Ok(("", raw))
}

fn valid_optional_port(port: &str) -> bool {
    port.strip_prefix(':')
        .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
}

/// `url.URL.Hostname` for an unbracketed host.
fn hostname(host: &str) -> &str {
    match host.rfind(':') {
        Some(colon) if valid_optional_port(&host[colon..]) => &host[..colon],
        _ => host,
    }
}

/// `net.ParseIP(host).IsLoopback()` for dotted-quad IPv4 (no leading zeros).
fn is_loopback_v4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let mut octets = [0_u8; 4];
    for (slot, part) in octets.iter_mut().zip(&parts) {
        if part.is_empty() || part.len() > 3 || (part.len() > 1 && part.starts_with('0')) {
            return false;
        }
        match part.parse::<u8>() {
            Ok(value) => *slot = value,
            Err(_) => return false,
        }
    }
    octets[0] == 127
}

/// Whether Go `time.ParseDuration` accepts `raw` (the flag package reports
/// every failure as `parse error`).
pub(super) fn valid_duration(raw: &[u8]) -> bool {
    const LIMIT: u64 = 1 << 63;
    let mut rest = raw;
    let negative = matches!(rest.first(), Some(b'-'));
    if matches!(rest.first(), Some(b'-' | b'+')) {
        rest = &rest[1..];
    }
    if rest == b"0" {
        return true;
    }
    if rest.is_empty() {
        return false;
    }
    let mut total = 0_u64;
    while !rest.is_empty() {
        if !(rest[0] == b'.' || rest[0].is_ascii_digit()) {
            return false;
        }
        let before = rest.len();
        let mut whole = 0_u64;
        while let Some(&digit) = rest.first().filter(|byte| byte.is_ascii_digit()) {
            if whole > LIMIT / 10 {
                return false;
            }
            whole = whole * 10 + u64::from(digit - b'0');
            if whole > LIMIT {
                return false;
            }
            rest = &rest[1..];
        }
        let pre = before != rest.len();
        let (mut fraction, mut scale, mut post) = (0_u64, 1_f64, false);
        if rest.first() == Some(&b'.') {
            rest = &rest[1..];
            let start = rest.len();
            let mut overflow = false;
            while let Some(&digit) = rest.first().filter(|byte| byte.is_ascii_digit()) {
                rest = &rest[1..];
                if overflow {
                    continue;
                }
                if fraction > (LIMIT - 1) / 10 {
                    overflow = true;
                    continue;
                }
                let next = fraction * 10 + u64::from(digit - b'0');
                if next > LIMIT {
                    overflow = true;
                    continue;
                }
                fraction = next;
                scale *= 10.0;
            }
            post = start != rest.len();
        }
        if !pre && !post {
            return false;
        }
        let unit_len = rest
            .iter()
            .position(|&byte| byte == b'.' || byte.is_ascii_digit())
            .unwrap_or(rest.len());
        if unit_len == 0 {
            return false;
        }
        let unit: u64 = match &rest[..unit_len] {
            b"ns" => 1,
            b"us" | b"\xc2\xb5s" | b"\xce\xbcs" => 1_000,
            b"ms" => 1_000_000,
            b"s" => 1_000_000_000,
            b"m" => 60_000_000_000,
            b"h" => 3_600_000_000_000,
            _ => return false,
        };
        rest = &rest[unit_len..];
        if whole > LIMIT / unit {
            return false;
        }
        let mut value = whole * unit;
        if fraction > 0 {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let extra = (fraction as f64 * (unit as f64 / scale)) as u64;
            value = value.saturating_add(extra);
            if value > LIMIT {
                return false;
            }
        }
        total = total.saturating_add(value);
        if total > LIMIT {
            return false;
        }
    }
    negative || total < LIMIT
}
