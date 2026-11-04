//! Dedicated additive-contract proof: the optional `history` field must stay invisible to every
//! frozen Go-recorded report. (Formerly this hashed `internal/usage/schema.go`; the Go source is
//! gone, so the frozen reports themselves are the evidence.)
use serde_json::Value;
use symbrain_usage::{LocalHistory, ProviderUsage};

const FROZEN_REPORTS: &[&str] = &[
    include_str!("fixtures/codex_authenticated_report.json"),
    include_str!("fixtures/claude_admin_authenticated_report.json"),
    include_str!("fixtures/kimi_authenticated_report.json"),
    include_str!("fixtures/nous_authenticated_report.json"),
    include_str!("fixtures/opencode_authenticated_report.json"),
    include_str!("fixtures/combined_native_authenticated_report.json"),
];

#[test]
fn additive_history_preserves_exact_frozen_go_schema_projection() {
    let mut providers = 0;
    for raw in FROZEN_REPORTS {
        let report: Value = serde_json::from_str(raw).expect("frozen Go report");
        for original in report["providers"].as_array().expect("providers") {
            providers += 1;
            let usage: ProviderUsage =
                serde_json::from_value(original.clone()).expect("provider parses");
            assert!(usage.history.is_none(), "history leaked into {original}");
            let rendered = serde_json::to_value(&usage).expect("provider renders");
            assert_eq!(&rendered, original, "legacy projection changed");
            assert!(rendered.get("history").is_none());
            // Go field order: id, display_name, configured, auth_status first.
            let text = serde_json::to_string(&usage).unwrap();
            assert!(text.starts_with(r#"{"id":""#), "{text}");
            let positions: Vec<usize> = ["\"display_name\"", "\"configured\"", "\"auth_status\""]
                .iter()
                .map(|key| text.find(key).expect("legacy key present"))
                .collect();
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
        }
    }
    assert!(
        providers >= 10,
        "only {providers} frozen providers replayed"
    );
}

#[test]
fn history_is_only_emitted_when_present() {
    let mut usage = ProviderUsage::default();
    assert!(
        !serde_json::to_string(&usage)
            .unwrap()
            .contains("\"history\"")
    );
    usage.history = Some(LocalHistory {
        scope: "local".into(),
        attribution: "test".into(),
        timezone: "UTC".into(),
        status: "ok".into(),
        excluded_events: 0,
        daily: Vec::new(),
        periods: Vec::new(),
    });
    assert!(
        serde_json::to_string(&usage)
            .unwrap()
            .contains("\"history\"")
    );
}
