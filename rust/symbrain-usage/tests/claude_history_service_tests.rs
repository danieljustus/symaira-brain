#![cfg(unix)]
#[path = "../../test-support/coverage.rs"]
mod coverage;
use std::{collections::BTreeMap, sync::Arc};
use symbrain_usage::{FixtureTransport, ProviderSelection, Response, Service, selected_providers};
#[test]
fn claude_history_consent_and_quota_independence() {
    if let Ok(mode) = std::env::var("CLAUDE_HISTORY_SERVICE_CHILD") {
        let providers = selected_providers(
            &ProviderSelection::parse(if mode == "unselected" {
                "grok"
            } else {
                "claude"
            })
            .unwrap(),
        );
        let transport = FixtureTransport::new(BTreeMap::from([(
            "claude".into(),
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
    std::fs::create_dir(root.path().join("projects")).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let source=serde_json::json!({"type":"assistant","timestamp":at,"requestId":"r","message":{"id":"m","role":"assistant","model":"claude-opus-5-5","stop_reason":"end_turn","content":"synthetic-private-sentinel","usage":{"input_tokens":0,"output_tokens":0}}}).to_string()+"\n";
    let path = root.path().join("projects/session.jsonl");
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
            .envs(coverage::profile_environment())
            .env("HOME", root.path())
            .env("CLAUDE_CONFIG_DIR", root.path())
            .env(
                "SYMBRAIN_USAGE_CLAUDE_HISTORY",
                match mode {
                    "disabled" => "0",
                    "nonexact" => "true",
                    _ => "1",
                },
            )
            .env(
                "SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY",
                if mode == "quota-error" { "0" } else { "1" },
            )
            .env("CLAUDE_HISTORY_SERVICE_CHILD", mode)
            .arg("claude_history_consent_and_quota_independence")
            .arg("--exact")
            .arg("--nocapture");
        if mode == "quota-error" {
            cmd.env("ANTHROPIC_OAUTH_TOKEN", "synthetic");
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
