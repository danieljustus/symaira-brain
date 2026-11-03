//! The HTTP exchange of the sync client: what Go's `http.Client` sends and
//! reports for one `Client.do` (`Go-http-client/1.1`, `Accept-Encoding:
//! gzip`, the bearer token, up to ten followed redirects, the 60 s client
//! timeout inside the run deadline) and the Go wording of its failures.

use super::sync_url::{self, Parsed};
use std::io::Read;
use std::time::{Duration, Instant};

/// `http.Client{Timeout: defaultTimeout}`.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 10;

/// One finished exchange (the final response after redirects).
pub(super) struct Exchange {
    pub status: u16,
    pub body: Vec<u8>,
    /// The failure that cut the body short, in Go's words.
    pub stop: Option<String>,
}

pub(super) struct Client {
    pub base: String,
    token: String,
    deadline: Instant,
}

enum Gate {
    /// The run deadline passed.
    Context,
    /// The per-request client timeout passed.
    Client,
}

impl Client {
    pub fn new(remote: &str, token: &str, deadline: Instant) -> Self {
        Self {
            base: remote.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            deadline,
        }
    }

    pub fn get(&self, target: &str) -> Result<Exchange, String> {
        self.send("GET", target, None)
    }

    pub fn post(&self, target: &str, body: Vec<u8>) -> Result<Exchange, String> {
        self.send("POST", target, Some(body))
    }

    /// `Client.do` up to the status check; `Err` carries the text of the
    /// wrapped error (`build request: ...` or `request <base>: <op> "<url>": ...`).
    fn send(&self, method: &str, target: &str, body: Option<Vec<u8>>) -> Result<Exchange, String> {
        let mut first =
            sync_url::parse(target).map_err(|reason| format!("build request: {reason}"))?;
        first.remove_empty_port();
        let started = Instant::now();
        let client_deadline = started + CLIENT_TIMEOUT;
        let op = if method == "GET" { "Get" } else { "Post" };
        let failed_raw = |shown: &str, cause: &str| {
            format!(
                "request {}: {op} {}: {cause}",
                self.base,
                symbrain_memory::sync_client::go_quote(shown)
            )
        };
        let failed = |url: &Parsed, cause: &str| failed_raw(&url.display(), cause);
        let mut url = first;
        let mut method = method.to_owned();
        let mut body = body;
        let mut referer: Option<String> = None;
        let mut authorization = true;
        let mut redirects = 0_usize;
        loop {
            let (deadline, gate) = if self.deadline <= client_deadline {
                (self.deadline, Gate::Context)
            } else {
                (client_deadline, Gate::Client)
            };
            let attempt = self.attempt(
                &method,
                &url,
                body.as_deref(),
                referer.as_deref(),
                authorization,
                deadline,
            );
            let mut reply = match attempt {
                Ok(reply) => reply,
                Err(Failure::Timeout) => {
                    let cause = match gate {
                        Gate::Context => "context deadline exceeded",
                        Gate::Client => {
                            "context deadline exceeded (Client.Timeout exceeded while awaiting headers)"
                        }
                    };
                    return Err(failed(&url, cause));
                }
                Err(Failure::Other(cause)) => return Err(failed(&url, &cause)),
            };
            let redirect = matches!(reply.status, 301 | 302 | 303 | 307 | 308);
            let location = reply.location.take().filter(|loc| !loc.is_empty());
            let Some(location) = location.filter(|_| redirect) else {
                return Ok(reply.finish(&gate, deadline));
            };
            // 307/308 keep the body; the others turn into a body-less GET.
            let keep_body = matches!(reply.status, 307 | 308);
            let next = resolve(&url, &location).map_err(|reason| {
                failed(
                    &url,
                    &format!(
                        "failed to parse Location header {}: {reason}",
                        symbrain_memory::sync_client::go_quote(&location)
                    ),
                )
            })?;
            // `defaultCheckRedirect`: ten requests are the most one call makes;
            // the error carries the raw `Location` text.
            if redirects + 1 >= MAX_REDIRECTS {
                return Err(failed_raw(&location, "stopped after 10 redirects"));
            }
            redirects += 1;
            if !matches!(next.scheme.as_str(), "http" | "https") {
                return Err(failed(
                    &next,
                    &format!(
                        "unsupported protocol scheme {}",
                        symbrain_memory::sync_client::go_quote(&next.scheme)
                    ),
                ));
            }
            if !keep_body {
                "GET".clone_into(&mut method);
                body = None;
            }
            referer = (url.scheme == "https" || next.scheme == "http")
                .then(|| referer_of(&url))
                .flatten();
            authorization = authorization && same_domain(url.hostname(), next.hostname());
            url = next;
        }
    }

    fn attempt(
        &self,
        method: &str,
        url: &Parsed,
        body: Option<&[u8]>,
        referer: Option<&str>,
        authorization: bool,
        deadline: Instant,
    ) -> Result<Reply, Failure> {
        if url.host.is_empty() {
            return Err(Failure::Other("http: no Host in request URL".to_owned()));
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(Failure::Timeout);
        };
        if self
            .token
            .bytes()
            .any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
        {
            return Err(Failure::Other(
                "net/http: invalid header field value for \"Authorization\"".to_owned(),
            ));
        }
        let host = url.hostname();
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        let mut builder = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .user_agent("Go-http-client/1.1")
            .accept("")
            .accept_encoding("gzip")
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(remaining));
        if loopback {
            builder = builder.proxy(None);
        }
        let agent: ureq::Agent = builder.build().into();
        let port = url.port();
        if port.parse::<u64>().is_ok_and(|number| number > 65535) || port.len() > 18 {
            return Err(Failure::Other(format!(
                "dial tcp: address {port}: invalid port"
            )));
        }
        let port = if port.is_empty() {
            if url.scheme == "https" { "443" } else { "80" }
        } else {
            port
        };
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let target = format!("{}://{authority}{}", url.scheme, url.request_uri());
        let mut request = ureq::http::Request::builder().method(method).uri(&target);
        if let Some(referer) = referer {
            request = request.header("Referer", referer);
        }
        if body.is_some() {
            request = request.header("Content-Type", "application/json");
        }
        if !self.token.is_empty() {
            if authorization {
                request = request.header("Authorization", format!("Bearer {}", self.token));
            }
        } else if authorization && let Some((name, password)) = &url.user {
            request = request.header(
                "Authorization",
                basic(name, password.as_deref().unwrap_or("")),
            );
        }
        let request = request
            .body(body.map(<[u8]>::to_vec).unwrap_or_default())
            .map_err(|error| Failure::Other(error.to_string()))?;
        match agent.run(request) {
            Ok(response) => {
                let status = response.status().as_u16();
                let header = |name: &str| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                let location = header("location");
                let gzip =
                    header("content-encoding").is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
                Ok(Reply {
                    status,
                    location,
                    gzip,
                    response: Some(response),
                })
            }
            Err(error) => Err(classify(&error, &authority, host)),
        }
    }
}

fn basic(name: &str, password: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("{name}:{password}");
    let mut out = String::from("Basic ");
    for chunk in raw.as_bytes().chunks(3) {
        let mut value = 0_u32;
        for (shift, byte) in [16, 8, 0].into_iter().zip(chunk) {
            value |= u32::from(*byte) << shift;
        }
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(TABLE[((value >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `refererForURL` of the previous request.
fn referer_of(url: &Parsed) -> Option<String> {
    if url.host.is_empty() {
        return None;
    }
    let mut stripped = url.clone();
    stripped.user = None;
    let text = stripped.display();
    Some(text.split('#').next().unwrap_or("").to_owned())
}

/// `shouldCopyHeaderOnRedirect` for `Authorization`: the same host or a
/// subdomain of the initial one.
fn same_domain(previous: &str, next: &str) -> bool {
    let canonical = |host: &str| host.trim_end_matches('.').to_ascii_lowercase();
    let (previous, next) = (canonical(previous), canonical(next));
    next == previous || next.ends_with(&format!(".{previous}"))
}

/// `URL.ResolveReference` for the reference forms a `Location` takes.
fn resolve(base: &Parsed, location: &str) -> Result<Parsed, String> {
    let reference = sync_url::parse(location)?;
    if !reference.scheme.is_empty() {
        return Ok(reference);
    }
    let absolute = if location.starts_with("//") {
        format!("{}:{location}", base.scheme)
    } else {
        let authority = base.display();
        let origin_end = authority.find("://").map_or(0, |at| {
            authority[at + 3..]
                .find('/')
                .map_or(authority.len(), |s| s + at + 3)
        });
        let origin = &authority[..origin_end];
        if location.starts_with('/') {
            format!("{origin}{location}")
        } else if location.starts_with('?') {
            format!("{origin}{}{location}", base.path)
        } else {
            let directory = base.path.rfind('/').map_or("/", |at| &base.path[..=at]);
            format!("{origin}{directory}{location}")
        }
    };
    let mut resolved = sync_url::parse(&absolute)?;
    resolved.path = clean_dots(&resolved.path);
    Ok(resolved)
}

fn clean_dots(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "." => {}
            ".." => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

enum Failure {
    Timeout,
    Other(String),
}

fn classify(error: &ureq::Error, authority: &str, host: &str) -> Failure {
    match error {
        ureq::Error::Timeout(ureq::Timeout::Connect) => Failure::Other(format!(
            "dial tcp {}: i/o timeout",
            dial_address(authority, host)
        )),
        ureq::Error::Timeout(_) => Failure::Timeout,
        ureq::Error::HostNotFound => {
            Failure::Other(format!("dial tcp: lookup {host}: no such host"))
        }
        ureq::Error::Io(io) => match io.kind() {
            std::io::ErrorKind::TimedOut => Failure::Timeout,
            std::io::ErrorKind::ConnectionRefused => Failure::Other(format!(
                "dial tcp {}: connect: connection refused",
                dial_address(authority, host)
            )),
            std::io::ErrorKind::AddrNotAvailable => Failure::Other(format!(
                "dial tcp {}: connect: {}",
                dial_address(authority, host),
                if cfg!(target_os = "macos") {
                    "can't assign requested address"
                } else {
                    "cannot assign requested address"
                }
            )),
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionAborted => {
                Failure::Other("EOF".to_owned())
            }
            std::io::ErrorKind::ConnectionReset => Failure::Other(format!(
                "read tcp {}: read: connection reset by peer",
                dial_address(authority, host)
            )),
            _ => {
                let text = io.to_string();
                if text.contains("failed to lookup address")
                    || text.contains("nodename nor servname")
                    || text.contains("Name or service not known")
                    || text.contains("No address associated")
                {
                    Failure::Other(format!("dial tcp: lookup {host}: no such host"))
                } else {
                    Failure::Other(text)
                }
            }
        },
        ureq::Error::ConnectionFailed => Failure::Other(format!(
            "dial tcp {}: connect: connection refused",
            dial_address(authority, host)
        )),
        other => {
            let text = other.to_string();
            if text.to_ascii_lowercase().contains("certificate") {
                Failure::Other(format!(
                    "tls: failed to verify certificate: x509: certificate signed by unknown authority ({text})"
                ))
            } else {
                Failure::Other(text)
            }
        }
    }
}

/// The address Go reports for a failed dial: the first resolved address.
fn dial_address(authority: &str, host: &str) -> String {
    use std::net::ToSocketAddrs;
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        if let std::net::IpAddr::V6(v6) = address
            && let Some(v4) = v6.to_ipv4_mapped()
        {
            let port = authority.rsplit(':').next().unwrap_or("");
            return format!("{v4}:{port}");
        }
        return authority.to_owned();
    }
    authority
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .map_or_else(|| authority.to_owned(), |address| address.to_string())
}

struct Reply {
    status: u16,
    location: Option<String>,
    gzip: bool,
    response: Option<ureq::http::Response<ureq::Body>>,
}

impl Reply {
    fn finish(mut self, gate: &Gate, deadline: Instant) -> Exchange {
        let mut body = Vec::new();
        let mut stop = None;
        if let Some(mut response) = self.response.take() {
            let mut reader = response.body_mut().as_reader();
            let mut raw = Vec::new();
            let mut chunk = [0_u8; 8192];
            // Success bodies are read through a 10 MiB limit, error bodies
            // through 64 KiB; the larger bound covers both.
            let limit = 10 << 20;
            loop {
                if raw.len() >= limit {
                    break;
                }
                let want = chunk.len().min(limit - raw.len());
                match reader.read(&mut chunk[..want]) {
                    Ok(0) => break,
                    Ok(count) => raw.extend_from_slice(&chunk[..count]),
                    Err(error) => {
                        stop = Some(match error.kind() {
                            std::io::ErrorKind::TimedOut => match gate {
                                Gate::Context => "context deadline exceeded".to_owned(),
                                Gate::Client => "context deadline exceeded (Client.Timeout or context cancellation while reading body)".to_owned(),
                            },
                            std::io::ErrorKind::UnexpectedEof => "unexpected EOF".to_owned(),
                            _ if Instant::now() >= deadline => "context deadline exceeded".to_owned(),
                            _ => error.to_string(),
                        });
                        break;
                    }
                }
            }
            if self.gzip {
                let (inflated, failure) = inflate(&raw);
                body = inflated;
                if stop.is_none() {
                    stop = failure;
                }
            } else {
                body = raw;
            }
        }
        Exchange {
            status: self.status,
            body,
            stop,
        }
    }
}

/// Go's transparent gzip body: the inflated bytes read before a failure.
fn inflate(raw: &[u8]) -> (Vec<u8>, Option<String>) {
    let mut out = Vec::new();
    let mut decoder = flate2::read::MultiGzDecoder::new(raw);
    match decoder.read_to_end(&mut out) {
        Ok(_) => (out, None),
        Err(error) => {
            let message = match error.kind() {
                std::io::ErrorKind::UnexpectedEof => "unexpected EOF".to_owned(),
                _ if out.is_empty() && raw.len() < 2 => "EOF".to_owned(),
                _ => {
                    let text = error.to_string();
                    if text.contains("invalid gzip header") {
                        "gzip: invalid header".to_owned()
                    } else {
                        text
                    }
                }
            };
            (out, Some(message))
        }
    }
}
