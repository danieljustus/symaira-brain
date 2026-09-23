//! Workspace extraction against the pinned Go net/url parser, not WHATWG URL.
use super::valid_percent_encoding;

pub(crate) fn normalize_workspace(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("wrk_") && trimmed.len() > 4 {
        return trimmed.to_string();
    }
    if let Some(path) = go_url_path(trimmed) {
        let mut parts = path.trim_matches('/').split('/').peekable();
        while let Some(part) = parts.next() {
            if part == "workspace"
                && let Some(candidate) = parts.peek()
                && candidate.starts_with("wrk_")
                && candidate.len() > 4
            {
                return (*candidate).to_string();
            }
        }
    }
    find_workspace_id(trimmed).unwrap_or_default()
}

pub(crate) fn extract_workspace(body: &[u8]) -> Option<String> {
    find_workspace_id(&String::from_utf8_lossy(body))
}

fn find_workspace_id(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for (index, window) in bytes.windows(4).enumerate() {
        if window == b"wrk_" {
            let length = bytes[index + 4..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric())
                .count();
            if length > 0 {
                return Some(text[index..index + 4 + length].to_string());
            }
        }
    }
    None
}

pub(crate) fn looks_signed_out(text: &str) -> bool {
    // Go uses simple Unicode case mapping; Rust's full lowercase expands İ.
    let lower: String = text
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    lower.contains("sign in")
        || lower.contains("unauthorized")
        || lower.contains("not authenticated")
}

fn go_url_path(raw: &str) -> Option<String> {
    let (before_fragment, fragment) = raw.split_once('#').unwrap_or((raw, ""));
    if before_fragment.bytes().any(|b| b < 0x20 || b == 0x7f) || !valid_percent_encoding(fragment) {
        return None;
    }
    let (scheme, remainder) = split_scheme(before_fragment)?;
    let mut rest = remainder.split('?').next().unwrap_or(remainder);
    if !rest.starts_with('/') {
        if !scheme.is_empty() {
            return Some(String::new());
        }
        if rest.split('/').next().unwrap_or(rest).contains(':') {
            return None;
        }
    }
    if rest.starts_with("//") && (!scheme.is_empty() || !rest.starts_with("///")) {
        let after_slashes = &rest[2..];
        let end = after_slashes.find('/').unwrap_or(after_slashes.len());
        if !valid_authority(&after_slashes[..end], scheme) {
            return None;
        }
        rest = &after_slashes[end..];
    }
    decode(rest)
}

fn split_scheme(raw: &str) -> Option<(&str, &str)> {
    for (i, b) in raw.bytes().enumerate() {
        match b {
            b':' if i == 0 => return None,
            b':' => return Some((&raw[..i], &raw[i + 1..])),
            b if b.is_ascii_alphabetic() => (),
            b if i > 0 && (b.is_ascii_digit() || b"+-.".contains(&b)) => (),
            _ => break,
        }
    }
    Some(("", raw))
}

fn decode(raw: &str) -> Option<String> {
    if !valid_percent_encoding(raw) {
        return None;
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let mut iter = raw.bytes();
    while let Some(byte) = iter.next() {
        bytes.push(if byte == b'%' {
            hex(iter.next()?) * 16 + hex(iter.next()?)
        } else {
            byte
        });
    }
    // Wire strings are UTF-8. Non-UTF-8 decoded URL bytes are not a native
    // parity claim; the credential/live-fetch Go fallback remains in force.
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn hex(b: u8) -> u8 {
    if b.is_ascii_digit() {
        b - b'0'
    } else {
        b.to_ascii_lowercase() - b'a' + 10
    }
}

fn valid_authority(authority: &str, scheme: &str) -> bool {
    let host = if let Some((user, host)) = authority.rsplit_once('@') {
        if !valid_percent_encoding(user)
            || !user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&b))
        {
            return false;
        }
        host
    } else {
        authority
    };
    if let Some(index) = host.rfind('[') {
        if index != 0 {
            return false;
        }
        let Some(end) = host.rfind(']') else {
            return false;
        };
        if !optional_port(&host[end + 1..]) {
            return false;
        }
        let literal = &host[1..end];
        let (address, zone) = literal
            .split_once("%25")
            .map_or((literal, None), |(a, z)| (a, Some(z)));
        if !valid_host_escapes(address, false) {
            return false;
        }
        let Some(address) = decode(address) else {
            return false;
        };
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        return zone.is_none_or(|zone| !zone.is_empty() && valid_host_escapes(zone, true));
    }
    if let Some(first) = host.find(':') {
        let index = if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            first
        } else {
            host.rfind(':').unwrap_or(first)
        };
        if !optional_port(&host[index..]) {
            return false;
        }
    }
    valid_host_escapes(host, false)
}

fn optional_port(port: &str) -> bool {
    port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|port| port.bytes().all(|b| b.is_ascii_digit()))
}

fn host_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:[]<>\"".contains(&byte)
}

fn valid_host_escapes(host: &str, zone: bool) -> bool {
    if !valid_percent_encoding(host) {
        return false;
    }
    let mut bytes = host.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let value = hex(bytes.next().unwrap()) * 16 + hex(bytes.next().unwrap());
            if value != b'%'
                && if zone {
                    value != b' ' && !host_byte(value)
                } else {
                    value < 128
                }
            {
                return false;
            }
        } else if byte < 128 && !host_byte(byte) {
            return false;
        }
    }
    true
}
