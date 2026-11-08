//! #996: the new default port and the refusal to bind next to a listener on
//! another address family. Every process runs in a disposable HOME/XDG root.
#![allow(clippy::pedantic)]

mod memory_serve_support;

use memory_serve_support::*;
use std::net::TcpListener;

#[test]
fn help_documents_the_new_default_and_the_guard() {
    let root = Root::new();
    let output = root
        .command(&rust_binary(), &["serve", "--help"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("(default 48787)"), "{text}");
    assert!(!text.contains("default 8787"), "{text}");
    assert!(text.contains("[::1] or the wildcard address"), "{text}");
}

#[test]
fn refuses_to_bind_next_to_an_ipv6_loopback_listener() {
    let Ok(blocker) = TcpListener::bind("[::1]:0") else {
        return; // IPv6 loopback unavailable on this host.
    };
    let port = blocker.local_addr().unwrap().port().to_string();
    let root = Root::new();
    let output = root
        .command(&rust_binary(), &["serve", "--port", &port])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "port {port} is already served on [::1] by another process; refusing to bind 127.0.0.1:{port}"
        )),
        "{stderr}"
    );
}

#[test]
fn same_family_clash_keeps_the_shipped_bind_error() {
    let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port().to_string();
    let root = Root::new();
    let output = root
        .command(&rust_binary(), &["serve", "--port", &port])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("failed to bind to 127.0.0.1:"), "{stderr}");
}
