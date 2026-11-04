//! Bounded rate, Host and CSRF checks before request-body reads.

use hyper::{HeaderMap, Method};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(super) fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

pub(super) fn loopback_host(authority: &str) -> bool {
    let host = if authority.parse::<IpAddr>().is_ok() {
        authority
    } else if authority.starts_with('[') {
        let Some(end) = authority.find(']') else {
            return false;
        };
        let rest = &authority[end + 1..];
        if !rest.is_empty() && (!rest.starts_with(':') || rest[1..].contains(':')) {
            return false;
        }
        &authority[1..end]
    } else if authority.matches(':').count() == 1 {
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
    } else {
        authority
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub(super) fn csrf_allowed(method: &Method, headers: &HeaderMap) -> bool {
    if ![Method::POST, Method::DELETE, Method::PUT, Method::PATCH].contains(method) {
        return true;
    }
    if header(headers, "authorization").starts_with("Bearer ")
        || header(headers, "x-requested-with").eq_ignore_ascii_case("XMLHttpRequest")
    {
        return true;
    }
    let origin = header(headers, "origin");
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|host| {
            loopback_host(host)
                && !host.contains('/')
                && (!host.to_ascii_lowercase().starts_with("localhost")
                    || host.starts_with("localhost"))
        })
}

pub(super) fn origin_allowed(origin: &str, host: &str, listener_port: u16) -> bool {
    origin.is_empty()
        || (listener_port != 0
            && loopback_host(host)
            && origin.strip_prefix("http://") == Some(host)
            && host
                .rsplit_once(':')
                .is_some_and(|(_, port)| port == listener_port.to_string()))
        || origin.starts_with("chrome-extension://")
        || origin.starts_with("moz-extension://")
}

/// One `net.ParseCIDR` network of `security.trusted_proxies`.
pub(super) struct Cidr {
    network: IpAddr,
    prefix: u32,
}

impl Cidr {
    pub fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = text.split_once('/')?;
        let network = address.parse::<IpAddr>().ok()?;
        let prefix = prefix.parse::<u32>().ok()?;
        let width = if network.is_ipv4() { 32 } else { 128 };
        (prefix <= width).then_some(Self { network, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let bits = |ip: IpAddr| -> Option<u128> {
            match ip {
                IpAddr::V4(v4) => Some(u128::from(u32::from(v4))),
                IpAddr::V6(v6) => Some(u128::from(v6)),
            }
        };
        if self.network.is_ipv4() != ip.is_ipv4() {
            return false;
        }
        let width = if ip.is_ipv4() { 32 } else { 128 };
        let shift = width - self.prefix;
        match (bits(self.network), bits(ip)) {
            (Some(a), Some(b)) => shift >= 128 || (a >> shift) == (b >> shift),
            _ => false,
        }
    }
}

#[derive(Default)]
pub(super) struct Limiter {
    buckets: Mutex<HashMap<String, Bucket>>,
    trusted: Vec<Cidr>,
}
struct Bucket {
    tokens: f64,
    seen: Instant,
}

impl Limiter {
    pub fn new(trusted: Vec<Cidr>) -> Self {
        Self {
            buckets: Mutex::default(),
            trusted,
        }
    }

    /// `RateLimiter.clientIP`: the direct peer, or the first forwarded
    /// address when the peer is a trusted proxy.
    pub fn key(&self, peer: IpAddr, headers: &HeaderMap) -> String {
        if self.trusted.iter().any(|cidr| cidr.contains(peer)) {
            let forwarded = header(headers, "x-forwarded-for");
            if !forwarded.is_empty() {
                let first = forwarded
                    .split_once(',')
                    .map_or(forwarded, |(first, _)| first.trim());
                if !first.is_empty() {
                    return first.to_owned();
                }
            }
            let real = header(headers, "x-real-ip");
            if !real.is_empty() {
                return real.to_owned();
            }
        }
        peer.to_string()
    }

    pub fn allow(&self, key: &str) -> bool {
        let Ok(mut buckets) = self.buckets.lock() else {
            return false;
        };
        let now = Instant::now();
        if buckets.len() >= 10000
            && !buckets.contains_key(key)
            && let Some(oldest) = buckets
                .iter()
                .min_by_key(|(_, bucket)| bucket.seen)
                .map(|(key, _)| key.clone())
        {
            buckets.remove(&oldest);
        }
        buckets.retain(|_, bucket| now.duration_since(bucket.seen) <= Duration::from_secs(600));
        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
            tokens: 20.0,
            seen: now,
        });
        bucket.tokens = (bucket.tokens
            + now.duration_since(bucket.seen).as_secs_f64() * (100.0 / 60.0))
            .min(20.0);
        bucket.seen = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }
}
