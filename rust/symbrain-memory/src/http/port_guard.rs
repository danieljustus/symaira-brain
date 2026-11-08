//! Refuses to bind `127.0.0.1:<port>` next to a listener that already serves
//! the same port on another address family or on the wildcard address (#996).
//!
//! macOS allows `127.0.0.1:P` to coexist with `*:P` or `[::1]:P`, after which
//! `http://127.0.0.1:P` and `http://localhost:P` (resolved through `::1`)
//! silently reach different servers. A short connect probe on both loopback
//! families detects such a listener before this process adds a second one.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// The loopback family that already answers on `port`, if any.
pub(super) fn existing_listener(port: u16) -> Option<&'static str> {
    [
        (IpAddr::V6(Ipv6Addr::LOCALHOST), "[::1]"),
        (IpAddr::V4(Ipv4Addr::LOCALHOST), "127.0.0.1"),
    ]
    .into_iter()
    .find(|(ip, _)| TcpStream::connect_timeout(&SocketAddr::new(*ip, port), PROBE_TIMEOUT).is_ok())
    .map(|(_, label)| label)
}

/// The error text for an `existing_listener` hit.
pub(super) fn conflict_message(port: u16, found: &str) -> String {
    format!(
        "port {port} is already served on {found} by another process; refusing to bind 127.0.0.1:{port} next to it because http://127.0.0.1:{port} and http://localhost:{port} would silently reach different servers (choose another --port)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn free_port_has_no_listener() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(existing_listener(port), None);
    }

    #[test]
    fn ipv6_loopback_listener_is_detected() {
        let Ok(listener) = TcpListener::bind("[::1]:0") else {
            return; // IPv6 loopback unavailable on this host.
        };
        let port = listener.local_addr().unwrap().port();
        assert_eq!(existing_listener(port), Some("[::1]"));
    }

    #[test]
    fn wildcard_listener_is_detected() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(existing_listener(port).is_some());
    }
}
