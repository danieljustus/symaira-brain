//! Go's early Host syntax boundary and one unchanged effective authority.

use hyper::{Method, Request, Version, header::HOST};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Rejection {
    Duplicate,
    Missing,
    Malformed,
}

impl Rejection {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Duplicate => "Bad Request",
            Self::Missing => "Bad Request: missing required Host header",
            Self::Malformed => "Bad Request: malformed Host header",
        }
    }
}

pub(super) fn effective_host<B>(request: &Request<B>) -> Result<&str, Rejection> {
    let mut hosts = request.headers().get_all(HOST).iter();
    let first = hosts.next();
    // Go rejects cardinality in readRequest, before middleware/auth/body reads.
    if hosts.next().is_some() {
        return Err(Rejection::Duplicate);
    }
    if first.is_none()
        && request.version() == Version::HTTP_11
        && request.method() != Method::CONNECT
    {
        return Err(Rejection::Missing);
    }
    let header = match first {
        Some(value) if value.as_bytes().iter().all(|byte| valid_host_byte(*byte)) => {
            value.to_str().map_err(|_| Rejection::Malformed)?
        }
        Some(_) => return Err(Rejection::Malformed),
        None => "",
    };
    // net/http selects URL.Host before Host. Remove URI userinfo as net/url
    // does, but retain the original host/port bytes: no IPv4/DNS canonicalizer
    // may turn a non-loopback spelling into an admitted loopback authority.
    Ok(request.uri().authority().map_or(header, |authority| {
        authority
            .as_str()
            .rsplit_once('@')
            .map_or(authority.as_str(), |(_, host)| host)
    }))
}

// Go1.26.7 httpguts.ValidHostHeader's byte set, not stricter DNS/port validation.
fn valid_host_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!$%&()*+,-.:;=[]'_~".contains(&byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::middleware::loopback_host;

    #[test]
    fn duplicates_precede_missing_auth_and_uri_authority() {
        let request = Request::builder()
            .uri("http://127.0.0.1:80/api/set")
            .method(Method::POST)
            .header(HOST, "127.0.0.1:80")
            .header(HOST, "untrusted.invalid")
            .body(())
            .unwrap();
        assert_eq!(effective_host(&request), Err(Rejection::Duplicate));
    }

    #[test]
    fn absolute_uri_keeps_required_header_and_empty_header_distinct() {
        let missing = Request::builder()
            .uri("http://127.0.0.1:80/api/list")
            .body(())
            .unwrap();
        assert_eq!(effective_host(&missing), Err(Rejection::Missing));
        let empty = Request::builder()
            .uri("http://127.0.0.1:80/api/list")
            .header(HOST, "")
            .body(())
            .unwrap();
        assert_eq!(effective_host(&empty), Ok("127.0.0.1:80"));
    }

    #[test]
    fn malformed_header_is_not_excused_by_an_absolute_uri() {
        let request = Request::builder()
            .uri("http://127.0.0.1:80/api/list")
            .header(HOST, "foreign@host")
            .body(())
            .unwrap();
        assert_eq!(effective_host(&request), Err(Rejection::Malformed));
    }

    #[test]
    fn absolute_host_wins_without_broadening_loopback_spellings() {
        for (authority, expected, admitted) in [
            ("untrusted.invalid", "untrusted.invalid", false),
            ("LOCALHOST:80", "LOCALHOST:80", true),
            ("127.1:80", "127.1:80", false),
            ("0x7f000001:80", "0x7f000001:80", false),
            ("%31%32%37.0.0.1:80", "%31%32%37.0.0.1:80", false),
            ("owned@127.0.0.1:80", "127.0.0.1:80", true),
        ] {
            let request = Request::builder()
                .uri(format!("http://{authority}/api/list"))
                .header(HOST, "127.0.0.1:99")
                .body(())
                .unwrap();
            let host = effective_host(&request).unwrap();
            assert_eq!(host, expected);
            assert_eq!(loopback_host(host), admitted);
        }
    }

    #[test]
    fn http10_and_connect_missing_host_exceptions_do_not_grant_loopback() {
        let older = Request::builder()
            .version(Version::HTTP_10)
            .body(())
            .unwrap();
        assert_eq!(effective_host(&older), Ok(""));
        assert!(!loopback_host(effective_host(&older).unwrap()));
        let tunnel = Request::builder()
            .method(Method::CONNECT)
            .uri("untrusted.invalid:80")
            .body(())
            .unwrap();
        assert_eq!(effective_host(&tunnel), Ok("untrusted.invalid:80"));
        assert!(!loopback_host(effective_host(&tunnel).unwrap()));
    }
}
