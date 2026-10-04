//! Go's pinned strict http(s) authority rules, including raw-byte userinfo.
// Source-derived compatibility logic: Go1.26.7 SDK, Go Authors, BSD-3-Clause.
// Retained license: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::{SyncError, escape, quoted};
pub(super) fn parse(raw: &str, scheme: &str) -> Result<String, SyncError> {
    let mut bytes;
    if raw.starts_with('[') {
        let close = raw
            .rfind(']')
            .ok_or_else(|| SyncError("missing ']' in host".into()))?;
        let port = &raw[close + 1..];
        check_port(port)?;
        let host = &raw[1..close];
        bytes = if let Some(zone) = host.find("%25") {
            let mut host = escape::unescape(&host[..zone], escape::Mode::Host)?;
            host.extend(escape::unescape(&raw[1 + zone..close], escape::Mode::Zone)?);
            host
        } else {
            escape::unescape(host, escape::Mode::Host)?
        };
        let decoded = String::from_utf8(bytes.clone())
            .map_err(|_| SyncError("native sync invalid-UTF8 host admission unproven".into()))?;
        let ip = decoded.split('%').next().unwrap_or("");
        if ip.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(SyncError(
                "native sync IP-literal diagnostic requires frozen Go proof".into(),
            ));
        }
        bytes.insert(0, b'[');
        bytes.push(b']');
        bytes.extend(port.as_bytes());
    } else {
        if raw.rfind('[').is_some() {
            return Err(SyncError("invalid IP-literal".into()));
        }
        if let Some(at) = raw.find(':') {
            let at = if scheme == "http" || scheme == "https" {
                at
            } else {
                raw.rfind(':').expect("colon")
            };
            check_port(&raw[at..])?;
        }
        bytes = escape::unescape(raw, escape::Mode::Host)?;
    }
    String::from_utf8(bytes)
        .map_err(|_| SyncError("native sync invalid-UTF8 host admission unproven".into()))
}
fn check_port(port: &str) -> Result<(), SyncError> {
    if !port.is_empty()
        && (!port.starts_with(':') || !port[1..].bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(SyncError(format!(
            "invalid port {} after host",
            quoted(port)
        )));
    }
    Ok(())
}
