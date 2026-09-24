use super::*;

#[test]
fn capabilities_are_truthful() {
    let c = canonical_capabilities();
    let actual: std::collections::BTreeSet<_> = c.interfaces.into_iter().collect();
    let expected = [
        "CookieEngine",
        "FrameManager",
        "InspectionEngine",
        "InteractionEngine",
        "NavigationStateProvider",
        "ScreenshotEngine",
        "TabManager",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(actual, expected);
}

#[test]
fn cookie_partition_uses_bidi_context_tag() {
    assert_eq!(
        context_partition("context-123"),
        json!({"type":"context","context":"context-123"})
    );
}

#[test]
fn unsupported_downloads_and_network_capture_are_typed() {
    for operation in ["downloads", "network.capture"] {
        assert!(matches!(
            FirefoxSession::unsupported(operation),
            FirefoxError::Unsupported { .. }
        ));
    }
}

#[test]
fn invalid_explicit_path_is_typed() {
    assert!(matches!(
        resolve_firefox_executable(Some(Path::new("/missing/firefox"))),
        Err(FirefoxError::Driver(_))
    ));
}
