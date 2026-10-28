#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_codex_history};
#[test]
fn codex_history_complete_json_corpus() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/codex-history-cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let root = tempfile::tempdir().unwrap();
        for (relative, source) in case["files"].as_object().unwrap() {
            let path = root.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let now = chrono::DateTime::parse_from_rfc3339(case["now"].as_str().unwrap()).unwrap();
        let history = scan_codex_history(root.path(), &now, "fixture", &Cancellation::default());
        let actual = serde_json::to_value(history).unwrap();
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        for (relative, source) in case["files"].as_object().unwrap() {
            assert_eq!(
                std::fs::read_to_string(root.path().join(relative)).unwrap(),
                source.as_str().unwrap()
            );
        }
        let output = actual.to_string();
        assert!(!output.contains("synthetic-private-sentinel"));
        assert!(!output.contains(root.path().to_str().unwrap()));
    }
}
