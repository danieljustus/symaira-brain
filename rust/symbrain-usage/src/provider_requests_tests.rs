use super::{trusted_https_url, validated_base};

#[test]
fn trusted_urls_reject_private_and_ambiguous_authorities() {
    for url in [
        "http://api.example.com/usage",
        "https://127.0.0.1/usage",
        "https://10.0.0.1/usage",
        "https://[::1]/usage",
        "https://user:pass@example.com/usage",
        "https://api.example.com:bad/usage",
        "https://api.example.com:0/usage",
        "https://api.example.com:65536/usage",
        "https://api.example.com.local/usage",
        "https://192.0.0.2/usage",
        "https://198.18.0.1/usage",
        "https://198.51.100.10/usage",
        "https://203.0.113.5/usage",
        "https://api.example.com\t/usage",
        "https://api.example.com/%zz",
    ] {
        assert!(
            !trusted_https_url(url, false),
            "accepted unsafe URL: {url:?}"
        );
    }
    assert!(trusted_https_url("https://api.example.com/usage", false));
    assert!(trusted_https_url(
        "https://api.example.com:65535/usage",
        false
    ));
    assert!(trusted_https_url("https://127.0.0.1/usage", true));
    assert!(trusted_https_url("https://[::1]/usage", true));
    assert!(!trusted_https_url("https://10.0.0.1/usage", true));
    assert!(trusted_https_url("https://198.51.99.1/usage", false));
}

#[test]
#[cfg(unix)]
fn cancelled_probe_kills_process_group_and_returns_within_bound() {
    use std::time::{Duration, Instant};

    let cancellation = crate::Cancellation::with_timeout(Duration::from_secs(5));
    cancellation.cancel();
    let started = Instant::now();
    let output = super::command_output_with_cancel(&cancellation, "sh", &["-c", "sleep 5"]);
    assert!(output.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
#[cfg(unix)]
fn timed_out_probe_kills_process_group_and_returns_within_bound() {
    use std::time::{Duration, Instant};

    let cancellation = crate::Cancellation::with_timeout(Duration::from_millis(20));
    let started = Instant::now();
    let output = super::command_output_with_cancel(&cancellation, "sh", &["-c", "sleep 5"]);
    assert!(output.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn invalid_provider_base_urls_fail_closed_to_defaults() {
    assert_eq!(
        validated_base("http://127.0.0.1:8080", "https://api.example.com"),
        "https://api.example.com"
    );
    assert_eq!(
        validated_base("https://api.example.com/", "https://fallback.example.com"),
        "https://api.example.com"
    );
}

#[test]
#[cfg(unix)]
fn probe_tool_resolution_fails_closed_for_executable_relative_path_entries() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    let root = tempfile::tempdir().expect("temporary probe path");
    let relative = root.path().join("relative");
    let absolute = root.path().join("absolute");
    std::fs::create_dir_all(&relative).expect("relative PATH directory");
    std::fs::create_dir_all(&absolute).expect("absolute PATH directory");
    let relative_tool = relative.join("ps");
    let absolute_tool = absolute.join("ps");
    std::fs::write(&relative_tool, "#!/bin/sh\n").expect("relative probe executable");
    std::fs::write(&absolute_tool, "#!/bin/sh\n").expect("absolute probe executable");
    std::fs::set_permissions(&relative_tool, std::fs::Permissions::from_mode(0o700))
        .expect("relative executable mode");
    std::fs::set_permissions(&absolute_tool, std::fs::Permissions::from_mode(0o700))
        .expect("absolute executable mode");

    assert_eq!(
        super::resolve_probe_tool_from(
            "ps",
            [PathBuf::from("relative"), absolute.clone()],
            root.path(),
        ),
        None,
        "Go LookPath would return ErrDot rather than selecting the later absolute executable"
    );
    assert_eq!(
        super::resolve_probe_tool_from(
            "ps",
            [PathBuf::from("missing"), absolute.clone()],
            root.path()
        ),
        Some(absolute_tool),
        "a relative directory without the command does not block a later absolute match"
    );
}

#[test]
#[cfg(unix)]
fn oversized_probe_output_terminates_the_child_while_it_is_running() {
    use std::time::{Duration, Instant};

    let cancellation = crate::Cancellation::with_timeout(Duration::from_secs(5));
    let started = Instant::now();
    let output = super::command_output_with_cancel(
        &cancellation,
        "sh",
        &["-c", "head -c 65537 /dev/zero; sleep 5"],
    );
    assert!(output.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// The request layer cannot be reached by the differential suite: a real fetch
/// needs a live endpoint and a working credential. `scripts/usage-request-oracle`
/// therefore records what the shipped strategies send, and this test pins the
/// port against that recording.
///
/// The historical fixture masks Go runtime metadata and random request ids.
/// The separate #620 oracle pins all five Kimi headers against a fresh process
/// at the Go 1.26.7 compatibility checkpoint. OpenCode's random id is compared
/// by shape in this historical fixture.
#[path = "request_oracle_tests.rs"]
mod request_oracle;
