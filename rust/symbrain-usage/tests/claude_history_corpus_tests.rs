#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_claude_history};
#[test]
fn claude_history_complete_json_corpus() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/claude-history-cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects/p/session/subagents");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("agent.jsonl");
        let source = case["source"].as_str().unwrap().as_bytes();
        std::fs::write(&path, source).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339(case["now"].as_str().unwrap()).unwrap();
        let history = scan_claude_history(root.path(), &now, "fixture", &Cancellation::default());
        let actual = serde_json::to_value(history).unwrap();
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        assert_eq!(std::fs::read(&path).unwrap(), source);
        let output = actual.to_string();
        assert!(!output.contains("synthetic-private-sentinel"));
        assert!(!output.contains(root.path().to_str().unwrap()));
    }
}
