//! Offline CLI expiry regressions; child HOME never sees owner credentials.
use super::coverage;

use crate::transport::{FixtureTransport, Response};
use std::sync::Arc;

#[test]
fn kimi_expiry_child() {
    let Ok(want) = std::env::var("KIMI_EXPIRY_EXPECTED") else {
        return;
    };
    let transport = FixtureTransport::with_sequences(
        [(
            "kimi".into(),
            vec![Ok(Response {
                status: 401,
                body: b"unauthorized".to_vec(),
                headers: std::collections::BTreeMap::new(),
            })],
        )]
        .into(),
    );
    let path = std::path::Path::new(&std::env::var("HOME").unwrap())
        .join(".kimi/credentials/kimi-code.json");
    let before = std::fs::read(&path).unwrap();
    let provider = super::kimi();
    assert!(provider.configured);
    let report =
        crate::Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    assert_eq!(report.providers[0].error.as_deref(), Some(want.as_str()));
    let count: usize = std::env::var("KIMI_EXPIRY_REQUESTS")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(transport.requests().len(), count);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn kimi_cli_expiry() {
    let expired = "Kimi Code CLI login expired — run the Kimi CLI once to refresh it";
    let rejected = "Kimi rejected the login (HTTP 401). Check the API access or sign in with the Kimi Code CLI again.";
    for (expiry, want, requests) in [
        (r#""2000-01-01T00:00:00Z""#, expired, "0"),
        ("946684800.5", expired, "0"),
        (r#""2999-01-01T00:00:00Z""#, rejected, "1"),
        ("32472144000", rejected, "1"),
        ("null", rejected, "1"),
        (r#""unknown""#, rejected, "1"),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".kimi/credentials/kimi-code.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!(r#"{{"access_token":"synthetic-cli","refresh_token":"do-not-touch","expires_at":{expiry}}}"#)).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "providers::provider_config::kimi_expiry_tests::kimi_expiry_child",
                "--nocapture",
            ])
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env(
                "KIMI_EXPIRY_EXPECTED",
                format!("all AI usage fallbacks failed: {want}"),
            )
            .env("KIMI_EXPIRY_REQUESTS", requests)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "expiry {expiry}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
