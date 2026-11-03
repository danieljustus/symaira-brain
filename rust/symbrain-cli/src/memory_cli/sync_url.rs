//! `net/url` as the sync client uses it: `url.Parse` (go1.27 semantics),
//! `URL.String`, `URL.Hostname`, `ValidateRemoteURL` and the request target
//! (`Host`, `RequestURI`) derived from a parsed URL. Every text is the one Go
//! prints; byte strings are handled as `String` (a non-UTF-8 spelling is
//! read lossily, which only changes how such an error would be quoted).

use std::fmt::Write as _;
use std::net::IpAddr;
use symbrain_memory::sync_client::go_quote;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    Host,
    Zone,
    UserPassword,
    Fragment,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Parsed {
    pub scheme: String,
    pub opaque: String,
    /// `(username, password)`; the password is `None` without a colon.
    pub user: Option<Credentials>,
    pub host: String,
    pub path: String,
    raw_path: String,
    pub force_query: bool,
    pub raw_query: String,
    fragment: String,
    raw_fragment: String,
    omit_host: bool,
}

fn should_escape(byte: u8, mode: Mode) -> bool {
    if byte.is_ascii_alphanumeric() {
        return false;
    }
    if matches!(mode, Mode::Host | Mode::Zone)
        && matches!(
            byte,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        )
    {
        return false;
    }
    match byte {
        b'-' | b'_' | b'.' | b'~' => return false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Mode::Path => return byte == b'?',
            Mode::UserPassword => return matches!(byte, b'@' | b'/' | b'?' | b':'),
            Mode::Fragment => return false,
            Mode::Host | Mode::Zone => {}
        },
        _ => {}
    }
    if mode == Mode::Fragment && matches!(byte, b'!' | b'(' | b')' | b'*') {
        return false;
    }
    true
}

fn quote_bytes(bytes: &[u8]) -> String {
    go_quote(&String::from_utf8_lossy(bytes))
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn unhex(byte: u8) -> u8 {
    9 * (byte >> 6) + (byte & 15)
}

fn escape_error(bytes: &[u8]) -> String {
    format!("invalid URL escape {}", quote_bytes(bytes))
}

fn unescape(text: &[u8], mode: Mode) -> Result<Vec<u8>, String> {
    let mut index = 0;
    let mut escapes = 0;
    while index < text.len() {
        match text[index] {
            b'%' => {
                escapes += 1;
                if index + 2 >= text.len() || !is_hex(text[index + 1]) || !is_hex(text[index + 2]) {
                    let tail = &text[index..];
                    return Err(escape_error(&tail[..tail.len().min(3)]));
                }
                let triple = &text[index..index + 3];
                if mode == Mode::Host && unhex(text[index + 1]) < 8 && triple != b"%25" {
                    return Err(escape_error(triple));
                }
                if mode == Mode::Zone {
                    let value = unhex(text[index + 1]) << 4 | unhex(text[index + 2]);
                    if triple != b"%25" && value != b' ' && should_escape(value, Mode::Host) {
                        return Err(escape_error(triple));
                    }
                }
                index += 3;
            }
            byte => {
                if matches!(mode, Mode::Host | Mode::Zone)
                    && byte < 0x80
                    && should_escape(byte, mode)
                {
                    return Err(format!(
                        "invalid character {} in host name",
                        quote_bytes(&[byte])
                    ));
                }
                index += 1;
            }
        }
    }
    if escapes == 0 {
        return Ok(text.to_vec());
    }
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'%' {
            out.push(unhex(text[index + 1]) << 4 | unhex(text[index + 2]));
            index += 3;
        } else {
            out.push(text[index]);
            index += 1;
        }
    }
    Ok(out)
}

fn escape(text: &[u8], mode: Mode) -> String {
    let mut out = String::with_capacity(text.len());
    for &byte in text {
        if should_escape(byte, mode) {
            let _ = write!(out, "%{byte:02X}");
        } else {
            out.push(char::from(byte));
        }
    }
    out
}

fn lossy(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

fn get_scheme(raw: &str) -> Result<(&str, &str), String> {
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
                    return Err("missing protocol scheme".to_owned());
                }
                return Ok((&raw[..index], &raw[index + 1..]));
            }
            _ => return Ok(("", raw)),
        }
    }
    Ok(("", raw))
}

fn valid_optional_port(port: &str) -> bool {
    match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(digits) => digits.bytes().all(|byte| byte.is_ascii_digit()),
    }
}

fn valid_userinfo(text: &str) -> bool {
    text.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '.'
                    | '_'
                    | ':'
                    | '~'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
                    | '@'
            )
    })
}

fn parse_host(scheme: &str, host: &str) -> Result<String, String> {
    if let Some(open) = host.rfind('[') {
        if open > 0 {
            return Err("invalid IP-literal".to_owned());
        }
        let Some(close) = host.rfind(']') else {
            return Err("missing ']' in host".to_owned());
        };
        let colon_port = &host[close + 1..];
        if !valid_optional_port(colon_port) {
            return Err(format!("invalid port {} after host", go_quote(colon_port)));
        }
        let unescaped_port = lossy(unescape(colon_port.as_bytes(), Mode::Host)?);
        let hostname = &host[open + 1..close];
        let unescaped = match hostname.find("%25") {
            Some(zone) => {
                let mut name = unescape(&hostname.as_bytes()[..zone], Mode::Host)?;
                name.extend(unescape(&hostname.as_bytes()[zone..], Mode::Zone)?);
                lossy(name)
            }
            None => lossy(unescape(hostname.as_bytes(), Mode::Host)?),
        };
        match parse_addr(&unescaped) {
            Err(reason) => return Err(format!("invalid host: {reason}")),
            Ok(true) => return Err("invalid IP-literal".to_owned()),
            Ok(false) => {}
        }
        return Ok(format!("[{unescaped}]{unescaped_port}"));
    }
    if let Some(first) = host.find(':') {
        let mut at = first;
        let last = host.rfind(':').unwrap_or(first);
        if last != first && !matches!(scheme, "http" | "https") {
            at = last;
        }
        let colon_port = &host[at..];
        if !valid_optional_port(colon_port) {
            return Err(format!("invalid port {} after host", go_quote(colon_port)));
        }
    }
    Ok(lossy(unescape(host.as_bytes(), Mode::Host)?))
}

/// `netip.ParseAddr` error text (`ParseAddr("in"): msg (at "x")`).
fn addr_error(input: &str, message: &str, at: &str) -> String {
    if at.is_empty() {
        format!("ParseAddr({}): {message}", go_quote(input))
    } else {
        format!(
            "ParseAddr({}): {message} (at {})",
            go_quote(input),
            go_quote(at)
        )
    }
}

fn parse_ipv4_fields(input: &str, start: usize, end: usize) -> Result<(), String> {
    let text = &input.as_bytes()[start..end];
    let (mut value, mut position, mut digits) = (0_u32, 0_u32, 0_u32);
    for (index, &byte) in text.iter().enumerate() {
        if byte.is_ascii_digit() {
            if digits == 1 && value == 0 {
                return Err(addr_error(
                    input,
                    "IPv4 field has octet with leading zero",
                    "",
                ));
            }
            value = value * 10 + u32::from(byte - b'0');
            digits += 1;
            if value > 255 {
                return Err(addr_error(input, "IPv4 field has value >255", ""));
            }
        } else if byte == b'.' {
            if index == 0 || index == text.len() - 1 || text[index - 1] == b'.' {
                return Err(addr_error(
                    input,
                    "IPv4 field must have at least one digit",
                    &String::from_utf8_lossy(&text[index..]),
                ));
            }
            if position == 3 {
                return Err(addr_error(input, "IPv4 address too long", ""));
            }
            position += 1;
            value = 0;
            digits = 0;
        } else {
            return Err(addr_error(
                input,
                "unexpected character",
                &String::from_utf8_lossy(&text[index..]),
            ));
        }
    }
    if position < 3 {
        return Err(addr_error(input, "IPv4 address too short", ""));
    }
    Ok(())
}

fn parse_ipv6(input: &str) -> Result<(), String> {
    let mut rest = input;
    let mut zone = "";
    if let Some(at) = rest.find('%') {
        zone = &rest[at + 1..];
        rest = &rest[..at];
        if zone.is_empty() {
            return Err(addr_error(input, "zone must be a non-empty string", ""));
        }
    }
    let mut ellipsis: i32 = -1;
    let mut filled = 0_usize;
    if rest.len() >= 2 && rest.starts_with("::") {
        ellipsis = 0;
        rest = &rest[2..];
        if rest.is_empty() {
            return Ok(());
        }
    }
    while filled < 16 {
        let bytes = rest.as_bytes();
        let mut offset = 0;
        let mut accumulator = 0_u32;
        while offset < bytes.len() {
            let Some(digit) = (bytes[offset] as char).to_digit(16) else {
                break;
            };
            accumulator = (accumulator << 4) + digit;
            if offset > 3 {
                return Err(addr_error(
                    input,
                    "each group must have 4 or less digits",
                    rest,
                ));
            }
            if accumulator > 0xffff {
                return Err(addr_error(input, "IPv6 field has value >=2^16", rest));
            }
            offset += 1;
        }
        if offset == 0 {
            return Err(addr_error(
                input,
                "each colon-separated field must have at least one digit",
                rest,
            ));
        }
        if offset < bytes.len() && bytes[offset] == b'.' {
            if ellipsis < 0 && filled != 12 {
                return Err(addr_error(
                    input,
                    "embedded IPv4 address must replace the final 2 fields of the address",
                    rest,
                ));
            }
            if filled + 4 > 16 {
                return Err(addr_error(
                    input,
                    "too many hex fields to fit an embedded IPv4 at the end of the address",
                    rest,
                ));
            }
            let mut end = input.len();
            if !zone.is_empty() {
                end -= zone.len() + 1;
            }
            parse_ipv4_fields(input, end - rest.len(), end)?;
            rest = "";
            filled += 4;
            break;
        }
        filled += 2;
        rest = &rest[offset..];
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with(':') {
            return Err(addr_error(input, "unexpected character, want colon", rest));
        } else if rest.len() == 1 {
            return Err(addr_error(
                input,
                "colon must be followed by more characters",
                rest,
            ));
        }
        rest = &rest[1..];
        if rest.starts_with(':') {
            if ellipsis >= 0 {
                return Err(addr_error(input, "multiple :: in address", rest));
            }
            ellipsis = i32::try_from(filled).unwrap_or(0);
            rest = &rest[1..];
            if rest.is_empty() {
                break;
            }
        }
    }
    if !rest.is_empty() {
        return Err(addr_error(input, "trailing garbage after address", rest));
    }
    if filled < 16 {
        if ellipsis < 0 {
            return Err(addr_error(input, "address string too short", ""));
        }
    } else if ellipsis >= 0 {
        return Err(addr_error(
            input,
            "the :: must expand to at least one field of zeros",
            "",
        ));
    }
    Ok(())
}

/// `netip.ParseAddr`; `Ok(true)` for a plain IPv4 address.
fn parse_addr(text: &str) -> Result<bool, String> {
    for byte in text.bytes() {
        match byte {
            b'.' => return parse_ipv4_fields(text, 0, text.len()).map(|()| true),
            b':' => return parse_ipv6(text).map(|()| false),
            b'%' => return Err(addr_error(text, "missing IPv6 address", "")),
            _ => {}
        }
    }
    Err(addr_error(text, "unable to parse IP", ""))
}

type Credentials = (String, Option<String>);

fn parse_authority(scheme: &str, authority: &str) -> Result<(Option<Credentials>, String), String> {
    let at = authority.rfind('@');
    let host = parse_host(scheme, at.map_or(authority, |at| &authority[at + 1..]))?;
    let Some(at) = at else {
        return Ok((None, host));
    };
    let userinfo = &authority[..at];
    if !valid_userinfo(userinfo) {
        return Err("net/url: invalid userinfo".to_owned());
    }
    let user = match userinfo.split_once(':') {
        None => (
            lossy(unescape(userinfo.as_bytes(), Mode::UserPassword)?),
            None,
        ),
        Some((name, password)) => (
            lossy(unescape(name.as_bytes(), Mode::UserPassword)?),
            Some(lossy(unescape(password.as_bytes(), Mode::UserPassword)?)),
        ),
    };
    Ok((Some(user), host))
}

fn parse_inner(raw: &str) -> Result<Parsed, String> {
    if raw.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return Err("net/url: invalid control character in URL".to_owned());
    }
    let mut url = Parsed::default();
    if raw == "*" {
        "*".clone_into(&mut url.path);
        return Ok(url);
    }
    let (scheme, mut rest) = get_scheme(raw)?;
    url.scheme = scheme.to_ascii_lowercase();
    if rest.ends_with('?') && rest.matches('?').count() == 1 {
        url.force_query = true;
        rest = &rest[..rest.len() - 1];
    } else if let Some((head, query)) = rest.split_once('?') {
        query.clone_into(&mut url.raw_query);
        rest = head;
    }
    if !rest.starts_with('/') {
        if !url.scheme.is_empty() {
            rest.clone_into(&mut url.opaque);
            return Ok(url);
        }
        if rest.split('/').next().unwrap_or("").contains(':') {
            return Err("first path segment in URL cannot contain colon".to_owned());
        }
    }
    if (!url.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
        let authority_and_rest = &rest[2..];
        let (authority, tail) = authority_and_rest
            .find('/')
            .map_or((authority_and_rest, ""), |at| {
                authority_and_rest.split_at(at)
            });
        let (user, host) = parse_authority(&url.scheme, authority)?;
        url.user = user;
        url.host = host;
        rest = tail;
    } else if !url.scheme.is_empty() && rest.starts_with('/') {
        url.omit_host = true;
    }
    let path = lossy(unescape(rest.as_bytes(), Mode::Path)?);
    if escape(path.as_bytes(), Mode::Path) == rest {
        url.raw_path.clear();
    } else {
        rest.clone_into(&mut url.raw_path);
    }
    url.path = path;
    Ok(url)
}

/// `url.Parse`; the error is the full `parse "<url>": <reason>` text.
pub(super) fn parse(raw: &str) -> Result<Parsed, String> {
    let (head, fragment) = raw
        .split_once('#')
        .map_or((raw, None), |(h, f)| (h, Some(f)));
    let mut url =
        parse_inner(head).map_err(|reason| format!("parse {}: {reason}", go_quote(head)))?;
    if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
        let value = unescape(fragment.as_bytes(), Mode::Fragment)
            .map_err(|reason| format!("parse {}: {reason}", go_quote(raw)))?;
        let value = lossy(value);
        if escape(value.as_bytes(), Mode::Fragment) == fragment {
            url.raw_fragment.clear();
        } else {
            fragment.clone_into(&mut url.raw_fragment);
        }
        url.fragment = value;
    }
    Ok(url)
}

fn split_host_port(host_port: &str) -> (&str, &str) {
    let mut host = host_port;
    let mut port = "";
    if let Some(colon) = host.rfind(':')
        && valid_optional_port(&host[colon..])
    {
        port = &host[colon + 1..];
        host = &host[..colon];
    }
    if host.starts_with('[') && host.ends_with(']') && host.len() >= 2 {
        host = &host[1..host.len() - 1];
    }
    (host, port)
}

impl Parsed {
    /// `removeEmptyPort`, applied by `http.NewRequest`.
    pub fn remove_empty_port(&mut self) {
        let colon = self.host.rfind(':');
        let bracket = self.host.rfind(']');
        if let Some(colon) = colon
            && bracket.is_none_or(|bracket| colon > bracket)
            && self.host.ends_with(':')
        {
            self.host.truncate(colon);
        }
    }

    /// `URL.Hostname`.
    pub fn hostname(&self) -> &str {
        split_host_port(&self.host).0
    }

    /// `URL.Port`.
    pub fn port(&self) -> &str {
        split_host_port(&self.host).1
    }

    fn escaped_path(&self) -> String {
        if !self.raw_path.is_empty()
            && valid_encoded(&self.raw_path, Mode::Path)
            && let Ok(decoded) = unescape(self.raw_path.as_bytes(), Mode::Path)
            && decoded == self.path.as_bytes()
        {
            return self.raw_path.clone();
        }
        if self.path == "*" {
            return "*".to_owned();
        }
        escape(self.path.as_bytes(), Mode::Path)
    }

    fn escaped_fragment(&self) -> String {
        if !self.raw_fragment.is_empty()
            && valid_encoded(&self.raw_fragment, Mode::Fragment)
            && let Ok(decoded) = unescape(self.raw_fragment.as_bytes(), Mode::Fragment)
            && decoded == self.fragment.as_bytes()
        {
            return self.raw_fragment.clone();
        }
        escape(self.fragment.as_bytes(), Mode::Fragment)
    }

    /// `URL.RequestURI`.
    pub fn request_uri(&self) -> String {
        let mut result = self.opaque.clone();
        if result.is_empty() {
            result = self.escaped_path();
            if result.is_empty() {
                "/".clone_into(&mut result);
            }
        } else if result.starts_with("//") {
            result = format!("{}:{result}", self.scheme);
        }
        if self.force_query || !self.raw_query.is_empty() {
            result.push('?');
            result.push_str(&self.raw_query);
        }
        result
    }

    /// `URL.String` with the password masked, as `url.Error` prints it.
    pub fn display(&self) -> String {
        let mut out = String::new();
        if !self.scheme.is_empty() {
            out.push_str(&self.scheme);
            out.push(':');
        }
        if self.opaque.is_empty() {
            if (!self.scheme.is_empty() || !self.host.is_empty() || self.user.is_some())
                && (!self.omit_host || !self.host.is_empty() || self.user.is_some())
            {
                if !self.host.is_empty() || !self.path.is_empty() || self.user.is_some() {
                    out.push_str("//");
                }
                if let Some((name, password)) = &self.user {
                    out.push_str(&escape(name.as_bytes(), Mode::UserPassword));
                    if password.is_some() {
                        out.push_str(":***");
                    }
                    out.push('@');
                }
                if !self.host.is_empty() {
                    out.push_str(&escape(self.host.as_bytes(), Mode::Host));
                }
            }
            let path = self.escaped_path();
            if !path.is_empty() && !path.starts_with('/') && !self.host.is_empty() {
                out.push('/');
            }
            if out.is_empty()
                && let Some(first) = path.split('/').next()
                && first.contains(':')
            {
                out.push_str("./");
            }
            out.push_str(&path);
        } else {
            out.push_str(&self.opaque);
        }
        if self.force_query || !self.raw_query.is_empty() {
            out.push('?');
            out.push_str(&self.raw_query);
        }
        if !self.fragment.is_empty() {
            out.push('#');
            out.push_str(&self.escaped_fragment());
        }
        out
    }
}

fn valid_encoded(text: &str, mode: Mode) -> bool {
    text.bytes().all(|byte| match byte {
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':'
        | b'@' | b'[' | b']' | b'%' => true,
        other => !should_escape(other, mode),
    })
}

/// `isLoopbackHost`.
fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => {
            address.is_loopback() || address.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

/// `syncclient.ValidateRemoteURL`: the message after
/// `symbrain memory sync: ` on failure.
pub(super) fn validate_remote(raw: &str, allow_insecure: bool) -> Result<(), String> {
    if raw.is_empty() {
        return Err("remote URL is required".to_owned());
    }
    let url = parse(raw).map_err(|reason| format!("parse remote URL: {reason}"))?;
    match url.scheme.as_str() {
        "https" => Ok(()),
        "http" => {
            let host = url.hostname();
            if host.is_empty() {
                return Err(format!(
                    "remote memory sync requires an https URL: http URL {} has no host",
                    go_quote(raw)
                ));
            }
            if is_loopback_host(host) || allow_insecure {
                return Ok(());
            }
            Err(format!(
                "remote memory sync requires an https URL: {raw} (pass --allow-insecure-http to override)"
            ))
        }
        "" => Err(format!(
            "parse remote URL: missing scheme in {}",
            go_quote(raw)
        )),
        other => Err(format!(
            "remote URL scheme {} is not supported (use https, or http for loopback)",
            go_quote(other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_go_parser_for_common_spellings() {
        let url = parse("http://user:pw@127.0.0.1:8080/base/?x=1#frag").unwrap();
        assert_eq!(url.hostname(), "127.0.0.1");
        assert_eq!(url.port(), "8080");
        assert_eq!(url.request_uri(), "/base/?x=1");
        assert_eq!(
            url.display(),
            "http://user:***@127.0.0.1:8080/base/?x=1#frag"
        );
        assert_eq!(
            parse("ht tp://x").unwrap_err(),
            "parse \"ht tp://x\": first path segment in URL cannot contain colon"
        );
        assert_eq!(
            parse("http://[::1").unwrap_err(),
            "parse \"http://[::1\": missing ']' in host"
        );
    }

    #[test]
    fn validation_wording() {
        assert_eq!(
            validate_remote("", false).unwrap_err(),
            "remote URL is required"
        );
        assert!(validate_remote("http://localhost:1", false).is_ok());
        assert!(validate_remote("http://[::1]:1", false).is_ok());
        assert_eq!(
            validate_remote("http://example.com", false).unwrap_err(),
            "remote memory sync requires an https URL: http://example.com (pass --allow-insecure-http to override)"
        );
        assert_eq!(
            validate_remote("ftp://x", false).unwrap_err(),
            "remote URL scheme \"ftp\" is not supported (use https, or http for loopback)"
        );
        assert_eq!(
            validate_remote("localhost:80", false).unwrap_err(),
            "remote URL scheme \"localhost\" is not supported (use https, or http for loopback)"
        );
    }
}
