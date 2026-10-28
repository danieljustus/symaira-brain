#![cfg(unix)]
use std::collections::BTreeMap;
use std::sync::Arc;
use symbrain_usage::{FixtureTransport, ProviderSelection, Response, Service, selected_providers};

// Child environment isolates opt-in and credential discovery from the developer's account.
#[test]
fn history_service_independent_of_quota() {
    if let Ok(mode) = std::env::var("GROK_HISTORY_TEST_MODE") {
        let selector = if mode == "unselected" {
            "devin"
        } else {
            "grok"
        };
        let providers = selected_providers(&ProviderSelection::parse(selector).unwrap());
        let transport = FixtureTransport::new(BTreeMap::from([(
            "grok".into(),
            Response {
                status: 412,
                body: br#"{"error":"No personal team"}"#.to_vec(),
                headers: BTreeMap::new(),
            },
        )]));
        let report = Service::with_transport(providers, Arc::new(transport.clone())).report();
        let p = &report.providers[0];
        if matches!(mode.as_str(), "disabled" | "unselected") {
            assert!(p.history.is_none());
        } else {
            assert_eq!(p.history.as_ref().unwrap().status, "available");
            assert!(!p.history.as_ref().unwrap().daily.is_empty());
            assert_eq!(p.configured, mode == "quota-error");
            assert_eq!(p.error.is_some(), mode == "quota-error");
            assert!(p.snapshot.is_none());
        }
        assert_eq!(
            transport.requests().len(),
            usize::from(mode == "quota-error")
        );
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let p = home.path().join("sessions/p");
    std::fs::create_dir_all(&p).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let event = serde_json::json!({"timestamp":now,"params":{"_meta":{"eventId":"synthetic"},"update":{"sessionUpdate":"turn_completed","usage":{"modelUsage":{"grok-build":{"inputTokens":0,"outputTokens":0,"costUsdTicks":0}}}}}});
    std::fs::write(p.join("updates.jsonl"), format!("{event}\n")).unwrap();
    for mode in ["missing-auth", "quota-error", "disabled", "unselected"] {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.env_clear()
            .env("HOME", home.path())
            .env("GROK_HOME", home.path())
            .env("GROK_HISTORY_TEST_MODE", mode)
            .arg("history_service_independent_of_quota")
            .arg("--exact")
            .arg("--nocapture");
        if mode != "disabled" {
            cmd.env("SYMBRAIN_USAGE_GROK_HISTORY", "1");
        }
        if mode == "quota-error" {
            cmd.env("GROK_ACCESS_TOKEN", "synthetic");
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
