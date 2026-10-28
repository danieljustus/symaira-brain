use super::*;
use crate::{Cancellation, FixtureTransport, Response, Service, Transport};
use std::collections::BTreeMap;
use std::sync::Arc;

const WEEKLY: &[u8] = br#"{"config":{"creditUsagePercent":23.5,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-07-03T04:01:09.238389+00:00","end":"2026-07-10T04:01:09.238389+00:00"},"onDemandCap":{"val":2500}}}"#;

#[test]
fn grok_transport_and_report() {
    let provider = grok_from_token(" fake-token ", "env".into(), chrono::Utc::now());
    let transport = FixtureTransport::new(BTreeMap::from([(
        "grok".into(),
        Response {
            status: 200,
            body: WEEKLY.into(),
            headers: BTreeMap::new(),
        },
    )]));
    let report = Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    assert!(report.providers[0].error.is_none());
    let snapshot = report.providers[0].snapshot.as_ref().unwrap();
    assert_eq!(snapshot.provider_id, "grok");
    assert_eq!(snapshot.source, "api");
    assert_eq!(snapshot.meters[0].used.as_deref(), Some("23.5"));
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.url,
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits"
    );
    assert_eq!(request.method, "GET");
    assert!(request.body.is_none());
    for (key, value) in [
        ("Authorization", "Bearer fake-token"),
        ("Accept", "application/json"),
        ("X-XAI-Token-Auth", "xai-grok-cli"),
        ("User-Agent", "symbrain"),
    ] {
        assert_eq!(request.headers.get(key).map(String::as_str), Some(value));
    }
}

#[test]
fn grok_http_failures_do_not_fabricate_or_leak() {
    let provider = grok_from_token("synthetic-token", "env".into(), chrono::Utc::now());
    for (status, body, expected) in [
        (401, "secret body", "grok login"),
        (403, "secret body", "grok login"),
        (
            412,
            r#"{"error":"No Personal Team exists"}"#,
            "no personal weekly quota",
        ),
        (412, r#"{"error":"other precondition"}"#, "HTTP 412"),
        (412, "no personal team", "HTTP 412"),
        (429, "secret body", "rate limited"),
        (500, "secret body", "HTTP 500"),
        (302, "secret body", "HTTP 302"),
        (200, "secret body", "malformed usage"),
    ] {
        let transport: Arc<dyn Transport> = Arc::new(FixtureTransport::new(BTreeMap::from([(
            "grok".into(),
            Response {
                status,
                body: body.as_bytes().into(),
                headers: BTreeMap::from([("Retry-After".into(), "5".into())]),
            },
        )])));
        let error = provider
            .fetch(&transport, &Cancellation::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{status}: {error}");
        assert!(!error.contains("secret body"));
    }
}

#[test]
fn grok_file_credentials_expiry_ambiguity_and_safety() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    for (body, status) in [
        (
            r#"{"a":{"key":" file-token ","refresh_token":"unused","expires_at":"2099-01-01T00:00:00Z"}}"#,
            "available",
        ),
        (
            r#"{"a":{"key":"old","expires":"2000-01-01T00:00:00Z"}}"#,
            "expired",
        ),
        (r#"{"a":{"key":"x.eyJleHAiOjF9.x"}}"#, "expired"),
        (r#"{"a":{"key":"one"},"b":{"key":"two"}}"#, "missing"),
        (r#"{"a":{"key":"bad\nvalue"}}"#, "missing"),
        (r#"{"a":{"key":true}}"#, "missing"),
        ("not json", "missing"),
    ] {
        fs::write(&path, body).unwrap();
        let provider = grok_from_file(&path, chrono::Utc::now());
        assert_eq!(provider.auth_status.status, status);
        assert_eq!(provider.configured, status == "available");
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
    }
    fs::write(
        &path,
        vec![b'x'; usize::try_from(MAX_CREDENTIAL_FILE_BYTES + 1).unwrap()],
    )
    .unwrap();
    assert!(!grok_from_file(&path, chrono::Utc::now()).configured);
    assert!(!grok_from_file(dir.path(), chrono::Utc::now()).configured);
    #[cfg(unix)]
    {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(!grok_from_file(&link, chrono::Utc::now()).configured);
    }
    for token in ["bad\r\nheader", "", "x.eyJleHAiOjF9.x"] {
        assert!(!grok_from_token(token, "env".into(), chrono::Utc::now()).configured);
    }
}

// Child process isolates environment mutation without unsafe set_var or real HOME reads.
#[test]
fn grok_environment_discovery() {
    if let Ok(expected) = env::var("GROK_TEST_EXPECTED") {
        let provider = grok();
        if expected == "missing" {
            assert!(!provider.configured);
            return;
        }
        assert!(provider.configured);
        assert_eq!(
            provider.auth_status.source.as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(provider.credential.as_deref(), Some("synthetic"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("custom");
    fs::create_dir(&custom).unwrap();
    fs::create_dir(dir.path().join(".grok")).unwrap();
    let body = r#"{"a":{"key":"synthetic"}}"#;
    fs::write(custom.join("auth.json"), body).unwrap();
    fs::write(dir.path().join(".grok/auth.json"), body).unwrap();
    for (token, override_home, expected) in [
        (None, None, "file"),
        (None, Some(&custom), "file"),
        (Some(" synthetic "), Some(&custom), "env"),
        (Some("env://GROK_TEST_TOKEN"), Some(&custom), "env"),
        (
            Some("env://ABSENT_GROK_TEST_TOKEN"),
            Some(&custom),
            "missing",
        ),
        (Some("bad\nheader"), Some(&custom), "missing"),
    ] {
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .env_clear()
            .env("HOME", dir.path())
            .env("USERPROFILE", dir.path())
            .env("GROK_TEST_EXPECTED", expected)
            .env("GROK_TEST_TOKEN", "synthetic")
            .arg("providers::provider_config::grok_tests::grok_environment_discovery")
            .arg("--exact")
            .arg("--nocapture");
        if let Some(value) = token {
            command.env("GROK_ACCESS_TOKEN", value);
        }
        if let Some(value) = override_home {
            command.env("GROK_HOME", value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(fs::read_to_string(custom.join("auth.json")).unwrap(), body);
    }
}
