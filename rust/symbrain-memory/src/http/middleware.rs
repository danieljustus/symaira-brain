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

#[derive(Default)]
pub(super) struct Limiter(Mutex<HashMap<IpAddr, Bucket>>);
struct Bucket {
    tokens: f64,
    seen: Instant,
}

impl Limiter {
    pub fn allow(&self, ip: IpAddr) -> bool {
        let Ok(mut buckets) = self.0.lock() else {
            return false;
        };
        let now = Instant::now();
        if buckets.len() >= 10000
            && !buckets.contains_key(&ip)
            && let Some(oldest) = buckets
                .iter()
                .min_by_key(|(_, bucket)| bucket.seen)
                .map(|(key, _)| *key)
        {
            buckets.remove(&oldest);
        }
        buckets.retain(|_, bucket| now.duration_since(bucket.seen) <= Duration::from_secs(600));
        let bucket = buckets.entry(ip).or_insert(Bucket {
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
