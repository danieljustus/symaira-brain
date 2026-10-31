#![cfg(unix)]
#[path = "../../test-support/coverage.rs"]
mod coverage;
use std::sync::Arc;
use symbrain_usage::{FixtureTransport, ProviderSelection, Service, selected_providers};
#[test]
fn claude_history_checkpoint_selected_report() {
    if std::env::var_os("CLAUDE_HISTORY_CHECKPOINT_CHILD").is_some() {
        let providers = selected_providers(&ProviderSelection::parse("claude").unwrap());
        let transport = FixtureTransport::default();
        let report = Service::with_transport(providers, Arc::new(transport.clone())).report();
        assert!(!report.providers[0].configured);
        assert!(transport.requests().is_empty());
        let h = report.providers[0].history.as_ref().unwrap();
        assert_eq!(h.status, "available");
        let m = &h.daily[0].models[0];
        assert_eq!(m.tokens.as_deref(), Some("190"));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("projects")).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let line=serde_json::json!({"type":"assistant","timestamp":at,"requestId":"req-synthetic","message":{"id":"msg-synthetic","role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":40}}}).to_string()+"\n";
    std::fs::write(root.path().join("projects/session.jsonl"), line).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", root.path())
        .env("CLAUDE_CONFIG_DIR", root.path())
        .env("SYMBRAIN_USAGE_CLAUDE_HISTORY", "1")
        .env("SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY", "1")
        .env("CLAUDE_HISTORY_CHECKPOINT_CHILD", "1")
        .arg("claude_history_checkpoint_selected_report")
        .arg("--exact")
        .arg("--nocapture")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
