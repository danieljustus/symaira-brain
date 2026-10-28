//! One loopback `GET` for the sync client, with the request the shipped Go
//! client sends (`Go-http-client/1.1`, `Accept-Encoding: gzip`, optional
//! bearer token, no redirects followed).
//!
//! Anything this module cannot classify with a proven wording is reported as
//! [`Fetched::Handoff`]; the caller then leaves the whole invocation to the
//! shipped implementation, which repeats the (read-only) request.

use std::io::Read;
use std::time::{Duration, Instant};

/// Why reading a response body stopped early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReadStop {
    /// The run deadline passed (`context deadline exceeded`).
    Deadline,
    /// The peer closed before `Content-Length` bytes (`unexpected EOF`).
    UnexpectedEof,
}

pub(super) enum Fetched {
    Reply {
        status: u16,
        body: Vec<u8>,
        /// Set when the body ended with a failure rather than a clean EOF.
        stopped: Option<ReadStop>,
    },
    /// The text Go reports after `Get "<url>": ` for a failed round trip.
    Transport(String),
    Handoff,
}

pub(super) fn get(url: &str, token: Option<&str>, deadline: Instant, body_limit: usize) -> Fetched {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return Fetched::Transport("context deadline exceeded".to_owned());
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .proxy(None)
        .user_agent("Go-http-client/1.1")
        .accept("")
        .accept_encoding("gzip")
        .timeout_global(Some(remaining))
        .build()
        .into();
    let mut request = agent.get(url);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let mut response = match request.call() {
        Ok(response) => response,
        Err(error) => return transport_failure(&error, url),
    };
    let status = response.status().as_u16();
    // Redirects are followed by the shipped client and compressed bodies are
    // inflated by it; neither is ported.
    if response.headers().contains_key("content-encoding")
        || (300..400).contains(&status) && !matches!(status, 300 | 304..=306)
        || status < 200
    {
        return Fetched::Handoff;
    }
    let mut reader = response.body_mut().as_reader();
    let mut body = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut stopped = None;
    loop {
        if body.len() >= body_limit {
            break;
        }
        let want = chunk.len().min(body_limit - body.len());
        match reader.read(&mut chunk[..want]) {
            Ok(0) => break,
            Ok(count) => body.extend_from_slice(&chunk[..count]),
            Err(error) => {
                stopped = Some(match error.kind() {
                    std::io::ErrorKind::TimedOut => ReadStop::Deadline,
                    std::io::ErrorKind::UnexpectedEof => ReadStop::UnexpectedEof,
                    _ if Instant::now() >= deadline => ReadStop::Deadline,
                    _ => return Fetched::Handoff,
                });
                break;
            }
        }
    }
    Fetched::Reply {
        status,
        body,
        stopped,
    }
}

fn transport_failure(error: &ureq::Error, url: &str) -> Fetched {
    match error {
        ureq::Error::Timeout(_) => Fetched::Transport("context deadline exceeded".to_owned()),
        // The refusal wording is the POSIX errno text; the Windows text differs.
        ureq::Error::Io(io)
            if io.kind() == std::io::ErrorKind::ConnectionRefused && !cfg!(windows) =>
        {
            let authority = url
                .strip_prefix("http://")
                .and_then(|rest| rest.split('/').next())
                .unwrap_or_default();
            Fetched::Transport(format!("dial tcp {authority}: connect: connection refused"))
        }
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Fetched::Transport("context deadline exceeded".to_owned())
        }
        _ => Fetched::Handoff,
    }
}

/// The Go duration helper for the one spelling family the native client
/// accepts: whole `s`, `m` or `h` counts of at least one second and at most
/// one minute.
pub(super) fn timeout_from_flag(raw: &[u8]) -> Option<Duration> {
    if raw.is_empty() {
        return Some(Duration::from_secs(60));
    }
    let text = std::str::from_utf8(raw).ok()?;
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = text.split_at(split);
    if digits.is_empty() || digits.len() > 3 || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    let count: u64 = digits.parse().ok()?;
    let seconds = match unit {
        "s" => count,
        "m" => count * 60,
        _ => return None,
    };
    (1..=60)
        .contains(&seconds)
        .then(|| Duration::from_secs(seconds))
}
