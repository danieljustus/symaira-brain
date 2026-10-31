use super::parse_snapshot;
use chrono::Utc;
use serde_json::Value;

#[test]
fn claude_current_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../tests/fixtures/claude-current-cases.json")).unwrap();
    for case in cases {
        let result = parse_snapshot(
            "claude",
            "oauth",
            &serde_json::to_vec(&case["body"]).unwrap(),
            Utc::now(),
        );
        if case["error"] == true {
            assert!(result.is_err());
            continue;
        }
        let snapshot = result.unwrap();
        assert_eq!(
            serde_json::to_value(snapshot.meters).unwrap(),
            case["meters"],
            "{}",
            case["name"]
        );
        assert_eq!(snapshot.currency.as_deref(), Some("USD"));
    }
}
