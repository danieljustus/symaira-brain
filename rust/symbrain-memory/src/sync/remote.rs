//! Pinned Go net/url policy, raw path ownership and redirect references.
//!
//! No WHATWG URL canonicalization is applied to the initial request. The
//! transport refuses unproved IDNA/zone/opaque URI families before sending.

mod escape;
mod host;
mod redirect;

use super::SyncError;
pub(super) use escape::query_escape;

pub(super) fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(ip) => ip.is_loopback(),
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
            }
        })
}

#[derive(Clone, Default)]
pub(super) struct Url {
    pub scheme: String,
    pub host: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
    pub user: Option<Vec<u8>>,
    pub password: Option<Vec<u8>>,
    pub opaque: String,
    pub force_query: bool,
}
/// Redact the entire authority userinfo even when percent decoding fails.
/// This is a bounded security divergence from frozen Go URL diagnostics.
pub(super) fn diagnostic(raw: &str) -> String {
    let Some(start) = raw.find("//").map(|at| at + 2) else {
        return raw.into();
    };
    let end = raw[start..]
        .find(['/', '?', '#'])
        .map_or(raw.len(), |at| start + at);
    let Some(at) = raw[start..end].rfind('@').map(|at| start + at) else {
        return raw.into();
    };
    format!("{}[redacted]{}", &raw[..start], &raw[at..])
}

fn quoted(raw: &str) -> String {
    serde_json::to_string(raw)
        .expect("UTF8 string")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
impl Url {
    pub fn parse(raw: &str) -> Result<Self, SyncError> {
        let (body, fragment) = raw.split_once('#').unwrap_or((raw, ""));
        let redact_error = |input: &str, error: SyncError| {
            let safe = diagnostic(input);
            if safe == input {
                SyncError(format!("parse {}: {error}", quoted(input)))
            } else {
                SyncError(format!(
                    "parse {}: invalid credential-bearing URL (details withheld)",
                    quoted(&safe)
                ))
            }
        };
        let mut value = Self::parse_body(body).map_err(|error| redact_error(body, error))?;
        escape::unescape(fragment, escape::Mode::Path).map_err(|error| redact_error(raw, error))?;
        value.fragment = fragment.into();
        Ok(value)
    }
    fn parse_body(raw: &str) -> Result<Self, SyncError> {
        if raw.bytes().any(|c| c < 32 || c == 127) {
            return Err(SyncError(
                "net/url: invalid control character in URL".into(),
            ));
        }
        let mut value = Self::default();
        let mut rest = raw;
        for (at, byte) in raw.bytes().enumerate() {
            if byte == b':' {
                if at == 0 {
                    return Err(SyncError("missing protocol scheme".into()));
                }
                value.scheme = raw[..at].to_ascii_lowercase();
                rest = &raw[at + 1..];
                break;
            }
            if !(byte.is_ascii_alphabetic()
                || (at > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.'))))
            {
                break;
            }
        }
        if rest.ends_with('?') && rest.bytes().filter(|c| *c == b'?').count() == 1 {
            value.force_query = true;
            rest = &rest[..rest.len() - 1];
        } else if let Some((path, query)) = rest.split_once('?') {
            rest = path;
            value.query = query.into();
        }
        if !rest.starts_with('/') && !value.scheme.is_empty() {
            value.opaque = rest.into();
            return Ok(value);
        }
        if value.scheme.is_empty()
            && !rest.starts_with('/')
            && rest.split('/').next().unwrap_or("").contains(':')
        {
            return Err(SyncError(
                "first path segment in URL cannot contain colon".into(),
            ));
        }
        if (!value.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
            let authority = &rest[2..];
            let (authority, path) = authority
                .split_once('/')
                .map_or((authority, ""), |(host, _)| {
                    (host, &authority[host.len()..])
                });
            rest = path;
            let (userinfo, hostname) = authority
                .rsplit_once('@')
                .map_or((None, authority), |(user, host)| (Some(user), host));
            value.host = host::parse(hostname, &value.scheme)?;
            if let Some(userinfo) = userinfo {
                if !userinfo
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:%@".contains(&c))
                {
                    return Err(SyncError("net/url: invalid userinfo".into()));
                }
                let (user, password) = userinfo
                    .split_once(':')
                    .map_or((userinfo, None), |(u, p)| (u, Some(p)));
                value.user = Some(escape::unescape(user, escape::Mode::Path)?);
                value.password = password
                    .map(|p| escape::unescape(p, escape::Mode::Path))
                    .transpose()?;
            }
        }
        let _ = escape::unescape(rest, escape::Mode::Path)?;
        value.path = escape::path(rest);
        Ok(value)
    }
    pub fn hostname(&self) -> &str {
        if self.host.starts_with('[') {
            return self
                .host
                .split(']')
                .next()
                .unwrap_or("")
                .trim_start_matches('[');
        }
        self.host
            .rsplit_once(':')
            .map_or(&self.host, |(host, port)| {
                if port.bytes().all(|c| c.is_ascii_digit()) {
                    host
                } else {
                    &self.host
                }
            })
    }
    pub fn uri(&self) -> Result<String, SyncError> {
        if !self.opaque.is_empty() || self.host.is_empty() {
            return Err(SyncError(
                "native sync opaque/no-host HTTP request family is not admitted".into(),
            ));
        }
        if !self.host.is_ascii() || self.host.contains('%') {
            return Err(SyncError(
                "native sync IDNA/IPv6-zone family is not admitted".into(),
            ));
        }
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        let query = if self.force_query || !self.query.is_empty() {
            format!("?{}", self.query)
        } else {
            String::new()
        };
        Ok(format!("{}://{}{path}{query}", self.scheme, self.host))
    }
    pub fn reference(&self, location: &str) -> Result<Self, SyncError> {
        redirect::resolve(self, location)
    }
    pub fn referer(&self) -> Result<String, SyncError> {
        let mut value = self.uri()?;
        if !self.fragment.is_empty() {
            value.push('#');
            value.push_str(&self.fragment);
        }
        Ok(value)
    }
    pub fn basic(&self) -> Option<Vec<u8>> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        self.user.as_ref().map(|user| {
            let mut plain = user.clone();
            plain.push(b':');
            plain.extend(self.password.as_deref().unwrap_or_default());
            format!("Basic {}", STANDARD.encode(plain)).into_bytes()
        })
    }
}

/// Frozen scheme/HTTPS/loopback guard. Does not authorize native routing.
/// # Errors
/// Returns the remote URL validation error before any network or cursor work.
pub fn validate_remote_url(raw: &str, allow_insecure: bool) -> Result<(), SyncError> {
    if raw.is_empty() {
        return Err(SyncError("remote URL is required".into()));
    }
    let url = Url::parse(raw).map_err(|error| SyncError(format!("parse remote URL: {error}")))?;
    match url.scheme.as_str() {
        "https" => Ok(()),
        "http" => {
            let host = url.hostname();
            if host.is_empty() {
                return Err(SyncError(format!(
                    "remote memory sync requires an https URL: http URL {} has no host",
                    quoted(&diagnostic(raw))
                )));
            }
            let loopback = is_loopback(host);
            if loopback || allow_insecure {
                Ok(())
            } else {
                Err(SyncError(format!(
                    "remote memory sync requires an https URL: {} (pass --allow-insecure-http to override)",
                    diagnostic(raw)
                )))
            }
        }
        "" => Err(SyncError(format!(
            "parse remote URL: missing scheme in {}",
            quoted(&diagnostic(raw))
        ))),
        scheme => Err(SyncError(format!(
            "remote URL scheme {} is not supported (use https, or http for loopback)",
            quoted(scheme)
        ))),
    }
}
