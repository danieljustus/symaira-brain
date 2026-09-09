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
