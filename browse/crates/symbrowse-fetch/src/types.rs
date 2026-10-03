//! Public honest transport requests, responses, errors and construction options.
use crate::retry::BackoffConfig;
use std::{collections::BTreeMap, error::Error, fmt, time::Duration};
use symbrowse_core::policy::Allowlist;

/// Honest is the only profile implemented by this transport slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Chrome,
    Firefox,
    Opera,
    Safari,
    Edge,
    Ios,
    Honest,
    Random,
}

impl Profile {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "firefox" => Self::Firefox,
            "opera" => Self::Opera,
            "safari" => Self::Safari,
            "edge" => Self::Edge,
            "ios" => Self::Ios,
            "honest" => Self::Honest,
            "random" => Self::Random,
            "" | "chrome" => Self::Chrome,
            _ => Self::Chrome,
        }
    }

    #[must_use]
    pub const fn is_browser_profile(self) -> bool {
        !matches!(self, Self::Honest)
    }

    /// Returns the stable warning emitted for an unknown profile. The
    /// transport deliberately does not impersonate browser TLS; callers use
    /// the Go escape hatch for these profiles until FETCH-002 is ported.
    #[must_use]
    pub fn parse_warning(value: &str) -> Option<String> {
        let known = matches!(
            value,
            "" | "chrome" | "firefox" | "opera" | "safari" | "edge" | "ios" | "honest" | "random"
        );
        (!known).then(|| format!("unknown profile, defaulting to chrome: {value}"))
    }
}

/// A single HTTP operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub user_agent: Option<String>,
    pub timeout: Option<Duration>,
    pub proxy: Option<String>,
    pub session: Option<String>,
    pub max_body: Option<usize>,
    pub max_compressed_body: Option<usize>,
    pub allow_private: bool,
    pub allowlist: Option<Allowlist>,
}

impl Request {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            url: url.into(),
            method: "POST".to_owned(),
            body: body.into(),
            ..Self::default()
        }
    }
}

impl Default for Request {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: "GET".to_owned(),
            headers: BTreeMap::new(),
            body: Vec::new(),
            user_agent: None,
            timeout: None,
            proxy: None,
            session: None,
            max_body: None,
            max_compressed_body: None,
            allow_private: false,
            allowlist: None,
        }
    }
}

/// The fetched response, with a UTF-8 normalized decoded body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub final_url: String,
    pub status_code: u16,
    pub headers: BTreeMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub protocol: String,
    pub content_type: Option<String>,
    pub elapsed: Duration,
    pub from_cache: bool,
}

/// A bounded-body error. `compressed` identifies which limit was exceeded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyTooLarge {
    pub url: String,
    pub limit: usize,
    pub compressed: bool,
}

impl fmt::Display for BodyTooLarge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "too_large: response from {} exceeds {} {} bytes",
            self.url,
            self.limit,
            if self.compressed {
                "compressed"
            } else {
                "decompressed"
            }
        )
    }
}

impl Error for BodyTooLarge {}

/// Errors returned by the honest HTTP client.
#[derive(Debug)]
pub enum FetchError {
    UnsupportedProfile(Profile),
    InvalidProxy(String),
    BlockedDomain(String),
    BlockedPrivate(String),
    InvalidRequest(String),
    Request(reqwest::Error),
    Archive(String),
    BodyTooLarge(BodyTooLarge),
    BodyRead(String),
    Decode(String),
    UnsupportedEncoding(String),
    Timeout,
    CircuitOpen(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProfile(profile) => {
                write!(formatter, "profile {profile:?} is not implemented")
            }
            Self::InvalidProxy(message) => write!(formatter, "invalid proxy: {message}"),
            Self::BlockedDomain(url) => write!(formatter, "blocked_domain: {url}"),
            Self::BlockedPrivate(url) => write!(formatter, "blocked_private: {url}"),
            Self::InvalidRequest(message) => write!(formatter, "invalid request: {message}"),
            Self::Request(error) => write!(formatter, "request failed: {error}"),
            Self::Archive(message) => write!(formatter, "archive request failed: {message}"),
            Self::BodyTooLarge(error) => error.fmt(formatter),
            Self::BodyRead(message) => write!(formatter, "read body: {message}"),
            Self::Decode(message) => write!(formatter, "decode body: {message}"),
            Self::UnsupportedEncoding(value) => {
                write!(formatter, "unsupported content encoding: {value}")
            }
            Self::Timeout => formatter.write_str("request timed out"),
            Self::CircuitOpen(host) => write!(formatter, "circuit breaker open for {host}"),
        }
    }
}

impl Error for FetchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::BodyTooLarge(error) => Some(error),
            _ => None,
        }
    }
}

/// Construction options for [`FetchClient`].
#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub timeout: Duration,
    pub max_body: usize,
    pub max_compressed_body: usize,
    pub retry: bool,
    pub backoff: BackoffConfig,
    pub rate: crate::RateLimitConfig,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_body: 10 * 1024 * 1024,
            max_compressed_body: 10 * 1024 * 1024,
            retry: false,
            backoff: BackoffConfig::default(),
            rate: crate::RateLimitConfig::default(),
        }
    }
}

impl ClientOptions {
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    #[must_use]
    pub fn max_body(mut self, limit: usize) -> Self {
        self.max_body = limit;
        self
    }
    #[must_use]
    pub fn max_compressed_body(mut self, limit: usize) -> Self {
        self.max_compressed_body = limit;
        self
    }
    #[must_use]
    pub fn retry(mut self, enabled: bool) -> Self {
        self.retry = enabled;
        self
    }
    #[must_use]
    pub fn backoff(mut self, config: BackoffConfig) -> Self {
        self.backoff = config;
        self
    }
    #[must_use]
    pub fn rate(mut self, config: crate::RateLimitConfig) -> Self {
        self.rate = config;
        self
    }
}
