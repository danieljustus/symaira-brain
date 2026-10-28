#![cfg(unix)]
use std::sync::Arc;
use symbrain_usage::{FixtureTransport, ProviderSelection, Service, selected_providers};
#[test]
fn codex_history_checkpoint_selected_report() {
    if std::env::var_os("CODEX_HISTORY_CHECKPOINT_CHILD").is_some() {
        let providers = selected_providers(&ProviderSelection::parse("codex").unwrap());
        let transport = FixtureTransport::default();
        let report = Service::with_transport(providers, Arc::new(transport.clone())).report();
        assert!(!report.providers[0].configured);
        assert!(transport.requests().is_empty());
        let h = report.providers[0].history.as_ref().unwrap();
        assert_eq!(h.status, "available");
        let m = &h.daily[0].models[0];
        assert_eq!(m.tokens.as_deref(), Some("120"));
        assert_eq!(m.recorded_cost, None);
        assert_eq!(m.estimated_cost.as_deref(), Some("0.000505"));
        assert!(h.price_snapshot.is_some());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("sessions")).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let line=serde_json::json!({"type":"turn_context","payload":{"model":"gpt-5.4"}}).to_string()+"\n"+&serde_json::json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":20,"reasoning_output_tokens":10,"total_tokens":120}}}}).to_string()+"\n";
    std::fs::write(root.path().join("sessions/session.jsonl"), line).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("HOME", root.path())
        .env("CODEX_HOME", root.path())
        .env("SYMBRAIN_USAGE_CODEX_HISTORY", "1")
        .env("SYMBRAIN_USAGE_CODEX_HISTORY_ONLY", "1")
        .env("CODEX_HISTORY_CHECKPOINT_CHILD", "1")
        .arg("codex_history_checkpoint_selected_report")
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
