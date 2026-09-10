//! Tests for activity tools default-deny behavior and explicit allow grants.

use symbrain_policy::constants::{MEMORY_MODE_READ_ONLY, SERVER_MEMORY};
use symbrain_policy::policy::Verdict;
use symbrain_policy::policy::eval::evaluate_preset;
use symbrain_policy::profile::ServerConfig;

#[test]
fn test_memory_activity_reads_default_deny_and_explicit_allow() {
    let mut cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_ONLY.to_string(),
        ..Default::default()
    };

    let report = evaluate_preset(SERVER_MEMORY, &cfg).expect("evaluate_preset failed");
    for &tool in &["activity_search", "activity_get", "activity_status"] {
        assert_eq!(
            report.verdict(tool),
            Verdict::Hidden,
            "expected {tool} to be hidden by default"
        );
    }

    cfg.tools_allow = vec![
        "activity_search".to_string(),
        "activity_get".to_string(),
        "activity_status".to_string(),
    ];

    let report2 = evaluate_preset(SERVER_MEMORY, &cfg).expect("evaluate_preset failed");
    assert_eq!(report2.exposed.len(), 3);
    for tool in &cfg.tools_allow {
        assert_eq!(report2.verdict(tool), Verdict::Exposed);
    }
}
