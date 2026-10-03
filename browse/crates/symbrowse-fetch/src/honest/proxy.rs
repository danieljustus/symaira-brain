//! Go-compatible HTTP environment routing, captured once per client.
use crate::FetchError;
use std::net::IpAddr;
use url::Url;

#[derive(Clone, Default)]
pub(crate) struct ProxyConfig {
    http: Option<Url>,
    https: Option<Url>,
    no_proxy: String,
    cgi: bool,
}

impl ProxyConfig {
    pub(crate) fn from_env() -> Self {
        fn first(upper: &str, lower: &str) -> String {
            [upper, lower]
                .into_iter()
                .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
                .unwrap_or_default()
        }
        Self {
            http: parse_proxy(&first("HTTP_PROXY", "http_proxy")),
            https: parse_proxy(&first("HTTPS_PROXY", "https_proxy")),
            no_proxy: first("NO_PROXY", "no_proxy"),
            cgi: std::env::var_os("REQUEST_METHOD").is_some_and(|v| !v.is_empty()),
        }
    }

    pub(crate) fn selected(
        &self,
        target: &Url,
        explicit: Option<&str>,
    ) -> Result<Option<Url>, FetchError> {
        if let Some(explicit) = explicit.filter(|v| !v.is_empty()) {
            return Url::parse(explicit)
                .map(Some)
                .map_err(|e| FetchError::InvalidProxy(e.to_string()));
        }
        let proxy = match target.scheme() {
            "http" => &self.http,
            "https" => &self.https,
            _ => return Ok(None),
        };
        if proxy.is_none() {
            return Ok(None);
        }
        // Go checks CGI before its localhost / NO_PROXY bypass.
        if target.scheme() == "http" && self.cgi {
            return Err(FetchError::InvalidProxy("refusing to use HTTP_PROXY value in CGI environment; see golang.org/s/cgihttpproxy".to_owned()));
        }
        let host = target
            .host_str()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        let ip = host.parse::<IpAddr>().ok().map(normalize_ip);
        if host == "localhost"
            || ip.is_some_and(loopback)
            || bypass(&self.no_proxy, &host, ip, target.port_or_known_default())
        {
            return Ok(None);
        }
        Ok(proxy.clone())
    }
}

fn parse_proxy(raw: &str) -> Option<Url> {
    if raw.is_empty() {
        return None;
    }
    // Do not reinterpret a malformed scheme URL as an unrelated host. Go's
    // percent-escape rejection also applies after its scheme-prefix attempt.
    if raw.as_bytes().iter().enumerate().any(|(i, byte)| {
        *byte == b'%'
            && (raw
                .as_bytes()
                .get(i + 1)
                .is_none_or(|v| !v.is_ascii_hexdigit())
                || raw
                    .as_bytes()
                    .get(i + 2)
                    .is_none_or(|v| !v.is_ascii_hexdigit()))
    }) {
        return None;
    }
    Url::parse(raw)
        .ok()
        .filter(|u| u.host_str().is_some())
        .or_else(|| {
            Url::parse(&format!("http://{raw}"))
                .ok()
                .filter(|u| u.host_str().is_some())
        })
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
        v4 => v4,
    }
}

fn loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
    }
}

fn bypass(raw: &str, host: &str, ip: Option<IpAddr>, port: Option<u16>) -> bool {
    raw.split(',').any(|entry| {
        let entry = entry.trim().to_ascii_lowercase();
        if entry == "*" {
            return true;
        }
        if let Some((network, prefix)) = entry.split_once('/') {
            return match (network.parse::<IpAddr>(), prefix.parse::<u32>(), ip) {
                (Ok(IpAddr::V4(a)), Ok(n @ 0..=32), Some(IpAddr::V4(b))) => {
                    n == 0 || (u32::from(a) >> (32 - n)) == (u32::from(b) >> (32 - n))
                }
                (Ok(IpAddr::V6(a)), Ok(n @ 0..=128), Some(IpAddr::V6(b))) => {
                    n == 0 || (u128::from(a) >> (128 - n)) == (u128::from(b) >> (128 - n))
                }
                _ => false,
            };
        }
        let (entry_host, entry_port) = if entry.starts_with('[') {
            let Some(end) = entry.find(']') else {
                return false;
            };
            let suffix = &entry[end + 1..];
            (
                &entry[1..end],
                if suffix.is_empty() {
                    None
                } else {
                    suffix.strip_prefix(':')
                },
            )
        } else if entry.matches(':').count() == 1 {
            let (h, p) = entry.split_once(':').expect("one colon");
            (h, Some(p))
        } else {
            (entry.as_str(), None)
        };
        if entry_port
            .filter(|p| !p.is_empty())
            .is_some_and(|p| p.parse::<u16>().ok() != port)
        {
            return false;
        }
        if let Ok(entry_ip) = entry_host.parse::<IpAddr>() {
            return ip == Some(normalize_ip(entry_ip));
        }
        if ip.is_some() || entry_host.is_empty() {
            return false;
        }
        let entry_host = entry_host
            .strip_prefix('*')
            .filter(|_| entry_host.starts_with("*."))
            .unwrap_or(entry_host);
        let normalized = if let Some(domain) = entry_host.strip_prefix('.') {
            url::Host::parse(domain).ok().map(|host| format!(".{host}"))
        } else {
            url::Host::parse(entry_host)
                .ok()
                .map(|host| host.to_string())
        };
        let Some(entry_host) = normalized.as_deref() else {
            return false;
        };
        if entry_host.starts_with('.') {
            return host.ends_with(entry_host);
        }
        host == entry_host
            || host
                .strip_suffix(entry_host)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}
