//! Prepared SDK-derived owner regressions; actual process/wire gates remain open.

use super::{RedirectAuthorization, Url, should_copy};

#[test]
fn sensitive_headers_use_literal_ascii_hostnames_and_sticky_strip() {
    for (initial, destination, allowed) in [
        ("localhost", "localhost", true),
        ("localhost", "LOCALHOST", false),
        ("LOCALHOST", "localhost", false),
        ("example.invalid", "sub.example.invalid", true),
        ("EXAMPLE.invalid", "sub.EXAMPLE.invalid", true),
        ("example.invalid", "sub.EXAMPLE.invalid", false),
        ("example.invalid", "evilexample.invalid", false),
        ("example.invalid", "example.invalid.evil", false),
        ("example.invalid", "example.invalid.", false),
        ("::1", "::1", true),
        ("::1", "::2", false),
        ("example.invalid", "::1%.example.invalid", false),
    ] {
        assert_eq!(should_copy(initial, destination), allowed);
    }
    let initial = Url::parse("http://localhost:1234/start").unwrap();
    let mut headers = RedirectAuthorization::new(b"owned-token");
    // Raw Host differs, but Go's delegation comparison ignores the port.
    let port = initial.reference("http://localhost:5678/next").unwrap();
    headers.redirect(&initial, &port);
    assert_eq!(
        headers.for_request(&port),
        Some(b"Bearer owned-token".to_vec())
    );
    let case = port.reference("http://LOCALHOST:1234/next").unwrap();
    headers.redirect(&initial, &case);
    assert_eq!(headers.for_request(&case), None);
    headers.redirect(&initial, &initial);
    assert_eq!(
        headers.for_request(&initial),
        None,
        "stripping stays sticky"
    );
}

#[test]
fn url_basic_is_per_send_while_original_bearer_has_precedence() {
    let initial = Url::parse("http://old:pass@localhost:1234/start").unwrap();
    let no_user = initial.reference("http://localhost:1234/next").unwrap();
    let new_user = initial
        .reference("http://new:word@localhost:1234/next")
        .unwrap();
    let relative = initial.reference("/relative").unwrap();
    let headers = RedirectAuthorization::new(b"");
    assert_eq!(
        headers.for_request(&initial),
        Some(b"Basic b2xkOnBhc3M=".to_vec())
    );
    assert_eq!(headers.for_request(&no_user), None);
    assert_eq!(
        headers.for_request(&new_user),
        Some(b"Basic bmV3OndvcmQ=".to_vec())
    );
    assert_eq!(
        headers.for_request(&relative),
        headers.for_request(&initial)
    );
    let raw = Url::parse("http://%ff:%80@localhost:1234/next").unwrap();
    assert_eq!(headers.for_request(&raw), Some(b"Basic /zqA".to_vec()));
    let mut headers = RedirectAuthorization::new(b"owned-token");
    assert_eq!(
        headers.for_request(&initial),
        Some(b"Bearer owned-token".to_vec())
    );
    assert_eq!(
        headers.for_request(&new_user),
        headers.for_request(&initial)
    );
    let foreign = initial
        .reference("http://new:word@127.0.0.1:1234/next")
        .unwrap();
    headers.redirect(&initial, &foreign);
    assert_eq!(
        headers.for_request(&foreign),
        Some(b"Basic bmV3OndvcmQ=".to_vec())
    );
    headers.redirect(&initial, &initial);
    assert_eq!(
        headers.for_request(&initial),
        Some(b"Basic b2xkOnBhc3M=".to_vec())
    );
}
