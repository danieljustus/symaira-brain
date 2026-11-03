//! The request validation Go's `net/http` client applies before any network
//! I/O, reproduced so credential and identity values that Go refuses to send
//! fail with the same report text instead of reaching (or breaking) the wire.
//!
//! Go checks every header in `http.Transport.roundTrip`: a name must be an RFC
//! 7230 token and a value may not contain a control byte other than space or
//! tab. The failure is wrapped by `http.Client` as `<Method> "<url>": <error>`.
//! Go iterates the header map in unspecified order, so when several headers are
//! invalid any of them is a Go-possible report; the sorted order used here is
//! deterministic.

use crate::{Cancellation, Request, Response, Transport};
use std::sync::Arc;

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(is_token_byte)
}

fn valid_header_value(value: &[u8]) -> bool {
    value
        .iter()
        .all(|byte| !(*byte < b' ' || *byte == 0x7f) || matches!(byte, b' ' | b'\t'))
}

/// `textproto.CanonicalMIMEHeaderKey`: names that are not tokens stay as given.
fn canonical_header_name(name: &str) -> String {
    if !valid_header_name(name) {
        return name.to_owned();
    }
    let mut upper = true;
    name.chars()
        .map(|character| {
            let mapped = if upper {
                character.to_ascii_uppercase()
            } else {
                character.to_ascii_lowercase()
            };
            upper = character == '-';
            mapped
        })
        .collect()
}

/// `strconv.Quote` for ASCII-only URLs; other characters keep Go's printable
/// rule via `char::escape_debug`, which agrees for the characters a trusted
/// HTTPS URL can contain.
fn go_quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                quoted.push_str(&format!("\\x{:02x}", u32::from(character)));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn operation(method: &str) -> String {
    let mut characters = method.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters.flat_map(char::to_lowercase)).collect()
    })
}

/// The text Go's client reports when it refuses `request` before connecting.
pub(crate) fn client_refusal(request: &Request) -> Option<String> {
    let mut names: Vec<&String> = request.headers.keys().collect();
    names.extend(request.header_bytes.keys().filter(|name| !request.headers.contains_key(*name)));
    names.sort();
    for name in names {
        let canonical = canonical_header_name(name);
        let reason = if !valid_header_name(&canonical) {
            Some(format!("net/http: invalid header field name {}", go_quote(&canonical)))
        } else {
            let value = request
                .header_bytes
                .get(name)
                .map(Vec::as_slice)
                .or_else(|| request.headers.get(name).map(String::as_bytes))
                .unwrap_or_default();
            (!valid_header_value(value)).then(|| {
                format!("net/http: invalid header field value for {}", go_quote(&canonical))
            })
        };
        if let Some(reason) = reason {
            return Some(format!(
                "{} {}: {reason}",
                operation(&request.method),
                go_quote(&request.url)
            ));
        }
    }
    None
}

/// Applies Go's client-side request refusal before delegating to `inner`, so
/// every provider flow (including OpenCode's own multi-request flow) reports
/// the same deterministic text instead of reaching the wire.
pub(crate) struct GoClient(pub(crate) Arc<dyn Transport>);

impl Transport for GoClient {
    fn request(&self, request: Request) -> Result<Response, String> {
        match client_refusal(&request) {
            Some(detail) => Err(detail),
            None => self.0.request(request),
        }
    }

    fn request_with_cancel(&self, request: Request, cancel: &Cancellation) -> Result<Response, String> {
        match client_refusal(&request) {
            Some(detail) => Err(detail),
            None => self.0.request_with_cancel(request, cancel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn request(headers: &[(&str, &str)]) -> Request {
        Request {
            header_bytes: BTreeMap::new(),
            provider_id: "copilot".into(),
            method: "GET".into(),
            url: "https://api.github.com/copilot_internal/user".into(),
            headers: headers.iter().map(|(n, v)| ((*n).into(), (*v).into())).collect(),
            body: None,
        }
    }

    #[test]
    fn go_header_validation_matches_httpguts() {
        assert_eq!(client_refusal(&request(&[("Authorization", "Bearer a b\tc")])), None);
        assert_eq!(client_refusal(&request(&[("X", "caf\u{e9}")])), None);
        for bad in ["a\u{1}b", "a\nb", "a\rb", "a\u{7f}b", "a\0b"] {
            assert_eq!(
                client_refusal(&request(&[("authorization", bad)])).as_deref(),
                Some("Get \"https://api.github.com/copilot_internal/user\": net/http: invalid header field value for \"Authorization\"")
            );
        }
        assert_eq!(
            client_refusal(&request(&[("bad name", "x")])).as_deref(),
            Some("Get \"https://api.github.com/copilot_internal/user\": net/http: invalid header field name \"bad name\"")
        );
        let mut raw = request(&[("X-Msh-Device-Id", "lossy")]);
        raw.header_bytes.insert("X-Msh-Device-Id".into(), vec![b'a', 0xff, b'b']);
        assert_eq!(client_refusal(&raw), None);
        raw.header_bytes.insert("X-Msh-Device-Id".into(), vec![b'a', 0x01]);
        assert!(client_refusal(&raw).is_some());
    }
}
