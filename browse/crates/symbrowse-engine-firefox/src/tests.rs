use super::*;
use std::path::Path;

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
fn evaluation_preserves_bidi_exception_details() {
    let result = super::evaluation_result(json!({
        "type": "exception",
        "exceptionDetails": {"text": "SecurityError: access denied"}
    }));
    assert_eq!(result.exception_text, "SecurityError: access denied");
    assert!(result.value.is_none());
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

#[test]
fn readiness_diagnostic_includes_browser_output_and_supported_channel() {
    let detail = startup_detail(
        b"",
        b"sandbox_extension_issue_file_to_process: denied\nSWGL mapping failed",
    )
    .expect("captured Firefox diagnostic");
    assert!(detail.contains("Mozilla Firefox Nightly"));
    assert!(detail.contains("sandbox_extension_issue_file_to_process"));
    assert!(detail.contains("SWGL mapping failed"));
}

#[test]
fn readiness_diagnostic_is_absent_without_browser_output() {
    assert_eq!(startup_detail(b"", b""), None);
}

#[tokio::test]
async fn startup_output_capture_keeps_only_bounded_tail() {
    let expected = vec![b'x'; startup::STARTUP_OUTPUT_LIMIT + 128];
    let output = Arc::new(Mutex::new(Vec::new()));
    capture_startup_output(std::io::Cursor::new(expected.clone()), Arc::clone(&output)).await;
    assert_eq!(
        captured_output(&output),
        expected[128..].to_vec(),
        "diagnostics must remain bounded while preserving the latest output"
    );
}
