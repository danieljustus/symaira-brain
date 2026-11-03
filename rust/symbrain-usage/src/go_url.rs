//! The subset of Go's `net/url` that provider endpoints depend on: the
//! `ParseRequestURI` trust check, `Parse` + `String` canonicalisation of a
//! request URL, and `http.NewRequest`'s empty-port removal. Custom provider
//! base URLs are user input, so the exact accept/reject decision and the exact
//! target Go would contact (escaped path, dropped fragment, preserved host
//! case) are part of the contract and are pinned by a Go-generated corpus.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    Host,
    Zone,
    Fragment,
}

fn should_escape(byte: u8, mode: Mode) -> bool {
    if byte.is_ascii_alphanumeric() {
        return false;
    }
    if matches!(mode, Mode::Host | Mode::Zone)
        && matches!(
            byte,
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'['
                | b']' | b'<' | b'>' | b'"'
        )
    {
        return false;
    }
    match byte {
        b'-' | b'_' | b'.' | b'~' => return false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => {
            return match mode {
                Mode::Path => byte == b'?',
                Mode::Fragment => false,
                Mode::Host | Mode::Zone => true,
            };
        }
        _ => {}
    }
    if mode == Mode::Fragment && matches!(byte, b'!' | b'(' | b')' | b'*') {
        return false;
    }
    true
}

fn escape(text: &str, mode: Mode) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if should_escape(byte, mode) {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push(char::from(byte));
        }
    }
    out
}

fn unhex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => byte - b'A' + 10,
    }
}

/// `url.unescape`; the bytes may be invalid UTF-8 only if the input was
/// escaped that way, which Go keeps as a Go string. Those are rejected here
/// (treated as a parse failure) because no supported endpoint needs them.
fn unescape(text: &str, mode: Mode) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len()
                    || !bytes[index + 1].is_ascii_hexdigit()
                    || !bytes[index + 2].is_ascii_hexdigit()
                {
                    return None;
                }
                if mode == Mode::Host && unhex(bytes[index + 1]) < 8 && &bytes[index..index + 3] != b"%25" {
                    return None;
                }
                if mode == Mode::Zone {
                    let value = unhex(bytes[index + 1]) << 4 | unhex(bytes[index + 2]);
                    if &bytes[index..index + 3] != b"%25" && should_escape(value, Mode::Host) {
                        return None;
                    }
                }
                out.push(unhex(bytes[index + 1]) << 4 | unhex(bytes[index + 2]));
                index += 3;
            }
            byte => {
                if matches!(mode, Mode::Host | Mode::Zone) && byte < 0x80 && should_escape(byte, mode) {
                    return None;
                }
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn valid_encoded(text: &str, mode: Mode) -> bool {
    text.bytes().all(|byte| match byte {
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@'
        | b'[' | b']' | b'%' => true,
        byte => !should_escape(byte, mode),
    })
}

fn valid_optional_port(port: &str) -> bool {
    match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(digits) => digits.bytes().all(|byte| byte.is_ascii_digit()),
    }
}

fn parse_host(host: &str) -> Option<String> {
    if host.starts_with('[') {
        let end = host.rfind(']')?;
        if !valid_optional_port(&host[end + 1..]) {
            return None;
        }
        if let Some(zone) = host[..end].find("%25") {
            let first = unescape(&host[..zone], Mode::Host)?;
            let second = unescape(&host[zone..end], Mode::Zone)?;
            let third = unescape(&host[end..], Mode::Host)?;
            return Some(first + &second + &third);
        }
    } else if let Some(colon) = host.rfind(':')
        && (!valid_optional_port(&host[colon..]) || host[..colon].contains(':'))
    {
        // Go 1.26 also rejects a second colon outside an IPv6 literal.
        return None;
    }
    unescape(host, Mode::Host)
}

fn valid_userinfo(text: &str) -> bool {
    text.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(
                character,
                '-' | '.' | '_' | ':' | '~' | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';'
                    | '=' | '%' | '@'
            )
    })
}

/// A parsed absolute `https`-style URL with an authority.
pub(crate) struct GoUrl {
    scheme: String,
    has_user: bool,
    host: String,
    path: String,
    raw_path: String,
    raw_query: String,
    force_query: bool,
    fragment: String,
    raw_fragment: String,
}

fn has_ctl(text: &str) -> bool {
    text.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
}

/// `url.Parse` (`via_request == false`) or `url.ParseRequestURI`
/// (`via_request == true`) restricted to URLs with a scheme and an authority;
/// anything else is reported as a failure because no caller accepts it.
fn parse(raw: &str, via_request: bool) -> Option<GoUrl> {
    if has_ctl(raw) || (via_request && raw.is_empty()) {
        return None;
    }
    let (body, fragment) = if via_request {
        (raw, None)
    } else {
        raw.split_once('#').map_or((raw, None), |(body, fragment)| (body, Some(fragment)))
    };
    let colon = body.find(|character: char| !(character.is_ascii_alphabetic()
        || character.is_ascii_digit() || matches!(character, '+' | '-' | '.')))?;
    if colon == 0 || body.as_bytes()[colon] != b':' || !body.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let scheme = body[..colon].to_ascii_lowercase();
    let mut rest = &body[colon + 1..];
    let mut raw_query = String::new();
    let mut force_query = false;
    if rest.ends_with('?') && rest.matches('?').count() == 1 {
        force_query = true;
        rest = &rest[..rest.len() - 1];
    } else if let Some((before, query)) = rest.split_once('?') {
        raw_query = query.to_owned();
        rest = before;
    }
    let authority_and_path = rest.strip_prefix("//")?;
    let (authority, path) = authority_and_path
        .find('/')
        .map_or((authority_and_path, ""), |index| authority_and_path.split_at(index));
    let (has_user, host_text) = match authority.rfind('@') {
        None => (false, authority),
        Some(index) => {
            let userinfo = &authority[..index];
            if !valid_userinfo(userinfo) {
                return None;
            }
            if !userinfo.contains(':') {
                unescape(userinfo, Mode::Path)?;
            }
            (true, &authority[index + 1..])
        }
    };
    let host = parse_host(host_text)?;
    let decoded = unescape(path, Mode::Path)?;
    let raw_path = if escape(&decoded, Mode::Path) == path { String::new() } else { path.to_owned() };
    let (fragment, raw_fragment) = match fragment {
        None => (String::new(), String::new()),
        Some(text) => {
            let decoded = unescape(text, Mode::Fragment)?;
            let raw = if escape(&decoded, Mode::Fragment) == text { String::new() } else { text.to_owned() };
            (decoded, raw)
        }
    };
    Some(GoUrl { scheme, has_user, host, path: decoded, raw_path, raw_query, force_query, fragment, raw_fragment })
}

fn split_host_port(host_port: &str) -> (String, String) {
    let mut host = host_port;
    let mut port = "";
    if let Some(colon) = host.rfind(':')
        && valid_optional_port(&host[colon..])
    {
        port = &host[colon + 1..];
        host = &host[..colon];
    }
    let host = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    (host.to_owned(), port.to_owned())
}

impl GoUrl {
    fn escaped_path(&self) -> String {
        if !self.raw_path.is_empty()
            && valid_encoded(&self.raw_path, Mode::Path)
            && unescape(&self.raw_path, Mode::Path).as_deref() == Some(self.path.as_str())
        {
            return self.raw_path.clone();
        }
        if self.path == "*" {
            return "*".into();
        }
        escape(&self.path, Mode::Path)
    }

    fn escaped_fragment(&self) -> String {
        if !self.raw_fragment.is_empty()
            && valid_encoded(&self.raw_fragment, Mode::Fragment)
            && unescape(&self.raw_fragment, Mode::Fragment).as_deref() == Some(self.fragment.as_str())
        {
            return self.raw_fragment.clone();
        }
        escape(&self.fragment, Mode::Fragment)
    }

    /// `URL.String()`.
    fn render(&self) -> String {
        let mut out = format!("{}:", self.scheme);
        if !self.host.is_empty() || !self.path.is_empty() || self.has_user {
            out.push_str("//");
        }
        if self.has_user {
            // Only an empty or opaque userinfo can reach here; callers reject
            // URLs with credentials before rendering.
            out.push('@');
        }
        if !self.host.is_empty() {
            out.push_str(&escape(&self.host, Mode::Host));
        }
        let path = self.escaped_path();
        if !path.is_empty() && !path.starts_with('/') && !self.host.is_empty() {
            out.push('/');
        }
        out.push_str(&path);
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

/// Go's `trustedHTTPSURL`: an HTTPS URL whose authority is not a local or
/// private-network origin (or, with `allow_loopback`, only a loopback IP).
pub(crate) fn trusted_https(raw: &str, allow_loopback: bool, private_ip: impl Fn(std::net::IpAddr) -> bool) -> bool {
    if raw.is_empty() || raw.trim() != raw || raw.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
        return false;
    }
    let Some(url) = parse(raw, true) else { return false };
    if url.scheme != "https" || url.host.is_empty() || url.has_user || !url.fragment.is_empty() {
        return false;
    }
    if url.host.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
        return false;
    }
    let (hostname, port) = split_host_port(&url.host);
    if !port.is_empty() {
        if !port.parse::<u16>().is_ok_and(|value| value != 0) {
            return false;
        }
    } else if url.host.contains(':') && !url.host.starts_with('[') {
        return false;
    }
    let host = hostname.to_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host == "local"
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
    {
        return false;
    }
    if let Some(ip) = parse_ip(host) {
        return if allow_loopback { ip.is_loopback() } else { !private_ip(ip) };
    }
    !allow_loopback
}

/// `netip.ParseAddr`, including a non-empty IPv6 zone.
fn parse_ip(host: &str) -> Option<std::net::IpAddr> {
    if let Some((address, zone)) = host.split_once('%') {
        if zone.is_empty() {
            return None;
        }
        address.parse::<std::net::Ipv6Addr>().ok().map(std::net::IpAddr::V6)
    } else {
        host.parse::<std::net::IpAddr>().ok()
    }
}

/// `http.NewRequest(url)`: `url.Parse`, empty-port removal and `String()`.
/// `None` when Go would refuse the URL.
pub(crate) fn request_url(raw: &str) -> Option<String> {
    let mut url = parse(raw, false)?;
    if let Some(colon) = url.host.rfind(':')
        && colon > url.host.rfind(']').map_or(0, |index| index)
        && url.host.ends_with(':')
    {
        url.host.truncate(colon);
    }
    Some(url.render())
}

/// RFC 3492 Punycode encoding of one label's code points.
fn punycode(label: &str) -> Option<String> {
    const BASE: u32 = 36;
    let input: Vec<u32> = label.chars().map(u32::from).collect();
    let mut output: String = label.chars().filter(char::is_ascii).collect();
    let basic = u32::try_from(output.len()).ok()?;
    let mut handled = basic;
    if basic > 0 {
        output.push('-');
    }
    let (mut n, mut delta, mut bias) = (128_u32, 0_u32, 72_u32);
    let digit = |value: u32| char::from(if value < 26 { b'a' + value as u8 } else { b'0' + (value - 26) as u8 });
    while usize::try_from(handled).ok()? < input.len() {
        let next = input.iter().copied().filter(|code| *code >= n).min()?;
        delta = delta.checked_add((next - n).checked_mul(handled + 1)?)?;
        n = next;
        for code in &input {
            if *code < n {
                delta = delta.checked_add(1)?;
            }
            if *code == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let threshold = if k <= bias { 1 } else if k >= bias + 26 { 26 } else { k - bias };
                    if q < threshold {
                        break;
                    }
                    output.push(digit(threshold + (q - threshold) % (BASE - threshold)));
                    q = (q - threshold) / (BASE - threshold);
                    k += BASE;
                }
                output.push(digit(q));
                // Bias adaptation.
                let mut scaled = if handled == basic { delta / 700 } else { delta / 2 };
                scaled += scaled / (handled + 1);
                let mut k = 0;
                while scaled > ((BASE - 1) * 26) / 2 {
                    scaled /= BASE - 1;
                    k += BASE;
                }
                bias = k + (BASE * scaled) / (scaled + 38);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(output)
}

/// The origin form a request URL takes on the wire: fragment removed, the
/// percent-escaped non-ASCII host converted to its Punycode (IDNA) form the way
/// Go's HTTP stack does before dialing. Labels are lowercased but not
/// NFC-normalised or otherwise remapped, which differs from Go only for exotic
/// compatibility characters in a user-supplied host.
pub(crate) fn wire_url(url: &str) -> Option<String> {
    let without_fragment = url.split('#').next()?;
    let (scheme, rest) = without_fragment.split_once("://")?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if !authority.contains('%') && authority.is_ascii() {
        return Some(without_fragment.to_owned());
    }
    let mut decoded = Vec::with_capacity(authority.len());
    let bytes = authority.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            decoded.push(unhex(bytes[index + 1]) << 4 | unhex(bytes[index + 2]));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let host = String::from_utf8(decoded).ok()?;
    let ascii = host
        .split('.')
        .map(|label| {
            if label.is_ascii() {
                Some(label.to_owned())
            } else {
                punycode(&label.to_lowercase()).map(|encoded| format!("xn--{encoded}"))
            }
        })
        .collect::<Option<Vec<_>>>()?
        .join(".");
    Some(format!("{scheme}://{ascii}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punycode_matches_rfc_3492_vectors() {
        assert_eq!(punycode("bücher").as_deref(), Some("bcher-kva"));
        assert_eq!(punycode("exämple").as_deref(), Some("exmple-cua"));
        assert_eq!(punycode("日本").as_deref(), Some("wgv71a"));
        assert_eq!(punycode("münchen").as_deref(), Some("mnchen-3ya"));
        assert_eq!(punycode("пример").as_deref(), Some("e1afmkfd"));
    }

    #[test]
    fn wire_url_drops_fragment_and_converts_idna_hosts() {
        assert_eq!(
            wire_url("https://ex%C3%A4mple.com/coding/v1/usages").as_deref(),
            Some("https://xn--exmple-cua.com/coding/v1/usages")
        );
        assert_eq!(
            wire_url("https://api.example.com/v1#/coding/v1/usages").as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(
            wire_url("https://API.example.com:8443/a%20b?q=1").as_deref(),
            Some("https://API.example.com:8443/a%20b?q=1")
        );
    }
}
