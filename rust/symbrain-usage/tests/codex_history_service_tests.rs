#![cfg(unix)]
use std::{collections::BTreeMap, sync::Arc};
use symbrain_usage::{FixtureTransport, ProviderSelection, Response, Service, selected_providers};
#[test]
fn codex_history_consent_and_quota_independence() {
    if let Ok(mode) = std::env::var("CODEX_HISTORY_SERVICE_CHILD") {
        let providers = selected_providers(
            &ProviderSelection::parse(if mode == "unselected" {
                "grok"
            } else {
                "codex"
            })
            .unwrap(),
        );
        let transport = FixtureTransport::new(BTreeMap::from([(
            "codex".into(),
            Response {
                status: 401,
                body: b"{}".to_vec(),
                headers: BTreeMap::new(),
            },
        )]));
        let report = Service::with_transport(providers, Arc::new(transport.clone())).report();
        let p = &report.providers[0];
        if mode == "enabled" || mode == "quota-error" {
            let h = p.history.as_ref().unwrap();
            assert_eq!(h.status, "available");
            assert_eq!(
                h.daily[0].models[0].estimated_cost.as_deref(),
                Some("0.0000025")
            );
        } else {
            assert!(p.history.is_none());
        }
        assert_eq!(
            transport.requests().len(),
            usize::from(mode == "quota-error")
        );
        if mode == "quota-error" {
            assert!(p.error.is_some());
            assert!(p.snapshot.is_none());
        }
        let output = serde_json::to_string(&report).unwrap();
        assert!(!output.contains("synthetic-private-sentinel"));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("sessions")).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let source=serde_json::json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":1,"output_tokens":0}}}}).to_string()+"\n";
    let path = root.path().join("sessions/session.jsonl");
    std::fs::write(&path, &source).unwrap();
    for mode in [
        "enabled",
        "disabled",
        "nonexact",
        "unselected",
        "quota-error",
    ] {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.env_clear()
            .env("HOME", root.path())
            .env("CODEX_HOME", root.path())
            .env(
                "SYMBRAIN_USAGE_CODEX_HISTORY",
                match mode {
                    "disabled" => "0",
                    "nonexact" => "true",
                    _ => "1",
                },
            )
            .env(
                "SYMBRAIN_USAGE_CODEX_HISTORY_ONLY",
                if mode == "quota-error" { "0" } else { "1" },
            )
            .env("CODEX_HISTORY_SERVICE_CHILD", mode)
            .arg("codex_history_consent_and_quota_independence")
            .arg("--exact")
            .arg("--nocapture");
        if mode == "quota-error" {
            cmd.env("CODEX_ACCESS_TOKEN", "synthetic");
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }
}
