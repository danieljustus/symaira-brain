//! Injectable HTTP GET for the API-backed importers (Paperless, Calendar).
//!
//! [`PlainHttp`] is a minimal std-only HTTP/1.1 client for `http://` URLs. It
//! cannot speak TLS: adding a TLS client dependency to this crate is a
//! separate decision, so an `https://` URL fails with a clear error and the
//! importer is exercised through [`HttpGet`] fakes. The Google Calendar API is
//! HTTPS-only, so the calendar importer needs a TLS-capable [`HttpGet`] from
//! its caller to run against the real service.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A completed response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Body bytes.
    pub body: Vec<u8>,
}

/// Performs a GET with the given headers.
pub trait HttpGet {
    /// # Errors
    /// A transport failure message (`API request failed: ...` is added by the caller).
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, String>;
}

/// Plain-HTTP client with a request timeout.
pub struct PlainHttp {
    /// Connect/read/write timeout.
    pub timeout: Duration,
}

impl Default for PlainHttp {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

fn dechunk(mut raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = raw
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("malformed chunked body")?;
        let size = std::str::from_utf8(&raw[..eol])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok())
            .ok_or("malformed chunk size")?;
        raw = &raw[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if raw.len() < size + 2 {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

impl HttpGet for PlainHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| format!("unsupported URL scheme (plain http only): {url}"))?;
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
        let address = if authority.contains(':') {
            authority.to_owned()
        } else {
            format!("{authority}:80")
        };
        let mut stream =
            TcpStream::connect(&address).map_err(|e| format!("dial tcp {address}: {e}"))?;
        for set in [
            stream.set_read_timeout(Some(self.timeout)),
            stream.set_write_timeout(Some(self.timeout)),
        ] {
            set.map_err(|e| e.to_string())?;
        }
        let mut request =
            format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("malformed HTTP response")?;
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .ok_or("malformed status line")?;
        let chunked = head.lines().any(|l| {
            l.to_ascii_lowercase().starts_with("transfer-encoding:")
                && l.to_ascii_lowercase().contains("chunked")
        });
        let body = &raw[split + 4..];
        Ok(Response {
            status,
            body: if chunked {
                dechunk(body)?
            } else {
                body.to_vec()
            },
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::{HttpGet, Response};
    use std::cell::RefCell;

    /// A recorded request: URL and headers.
    pub(crate) type Call = (String, Vec<(String, String)>);

    /// Canned replies keyed by a URL substring; records every request.
    pub(crate) struct FakeHttp {
        pub replies: Vec<(&'static str, u16, String)>,
        pub calls: RefCell<Vec<Call>>,
    }

    impl FakeHttp {
        pub(crate) fn new(replies: Vec<(&'static str, u16, String)>) -> Self {
            Self {
                replies,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl HttpGet for &FakeHttp {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
            self.calls.borrow_mut().push((
                url.to_owned(),
                headers
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect(),
            ));
            self.replies
                .iter()
                .find(|(needle, _, _)| url.contains(needle))
                .map(|(_, status, body)| Response {
                    status: *status,
                    body: body.clone().into_bytes(),
                })
                .ok_or_else(|| format!("unexpected url {url}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn serve(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 4096];
            let n = stream.read(&mut buffer).unwrap();
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&buffer[..n]).into_owned()
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    #[test]
    fn plain_and_chunked_bodies() {
        let (base, server) = serve("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        let response = PlainHttp::default()
            .get(
                &format!("{base}/api/x?a=1"),
                &[("Authorization", "Token t")],
            )
            .unwrap();
        assert_eq!(
            (response.status, response.body.as_slice()),
            (200, &b"{}"[..])
        );
        let request = server.join().unwrap();
        assert!(
            request.starts_with("GET /api/x?a=1 HTTP/1.1\r\n")
                && request.contains("Authorization: Token t\r\n"),
            "{request}"
        );
        let (base, server) = serve(
            "HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        );
        let response = PlainHttp::default().get(&format!("{base}/"), &[]).unwrap();
        assert_eq!(
            (response.status, response.body.as_slice()),
            (404, &b"abcde"[..])
        );
        server.join().unwrap();
    }

    #[test]
    fn https_is_refused_clearly() {
        let error = PlainHttp::default()
            .get("https://example.invalid/x", &[])
            .unwrap_err();
        assert!(error.starts_with("unsupported URL scheme"), "{error}");
    }
}
