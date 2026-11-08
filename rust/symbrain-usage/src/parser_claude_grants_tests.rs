//! Offline fixtures for the read-only reset-grant block (#999).
use crate::parser::parse_snapshot;
use chrono::{DateTime, Utc};

fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn report(extra: &str) -> (crate::UsageSnapshot, String) {
    let body = format!(r#"{{"five_hour":{{"utilization":10}}{extra}}}"#);
    let snapshot = parse_snapshot("claude", "oauth", body.as_bytes(), now()).expect("snapshot");
    let wire = serde_json::to_string(&snapshot).expect("wire");
    (snapshot, wire)
}

#[test]
fn present_block_is_reported_with_exact_wire_bytes() {
    let (snapshot, wire) = report(
        r#","cedar_ember":{"eligible":true,"next_grant_id":"g1","grants":[
        {"id":"g1","resets_left":2,"ends_at":"2026-12-01T00:00:00Z","clears":["five_hour","seven_day"]},
        {"id":"g2","resets_left":-3.7,"clears":["five_hour",7]}]}"#,
    );
    let status = snapshot.reset_grants.expect("grants");
    assert!(status.eligible);
    assert_eq!(status.next_grant_id.as_deref(), Some("g1"));
    assert_eq!(status.grants[1].resets_left, 0);
    assert!(
        status.grants[1].clears.is_empty(),
        "mixed clears are dropped whole"
    );
    assert!(wire.contains(concat!(
        r#""reset_grants":{"eligible":true,"grants":["#,
        r#"{"id":"g1","resets_left":2,"ends_at":"2026-12-01T00:00:00Z","clears":["five_hour","seven_day"]},"#,
        r#"{"id":"g2","resets_left":0,"clears":[]}],"next_grant_id":"g1"}"#
    )), "{wire}");
    assert_eq!(snapshot.meters.len(), 1);
}

#[test]
fn absent_null_and_malformed_blocks_leave_the_rest_of_the_report_intact() {
    let (baseline, baseline_wire) = report("");
    assert!(!baseline_wire.contains("reset_grants"));
    for block in [
        r#","cedar_ember":null"#,
        r#","cedar_ember":"eligible""#,
        r#","cedar_ember":[]"#,
        r#","cedar_ember":{"eligible":true,"grants":[{"id":"","resets_left":1}]}"#,
        r#","cedar_ember":{"eligible":true,"grants":[{"id":"g1"}]}"#,
        r#","cedar_ember":{"eligible":true,"grants":[{"id":"g1","resets_left":"many"}]}"#,
        r#","cedar_ember":{"eligible":true,"grants":[{"id":"ok","resets_left":1},"x"]}"#,
    ] {
        let (snapshot, wire) = report(block);
        assert_eq!(snapshot.reset_grants, None, "{block}");
        assert_eq!(wire, baseline_wire, "{block}");
        assert_eq!(snapshot.meters, baseline.meters, "{block}");
    }
}

#[test]
fn ineligible_and_grantless_blocks_are_reported_as_such() {
    let (snapshot, wire) = report(r#","cedar_ember":{"grants":"none"}"#);
    let status = snapshot.reset_grants.expect("object block");
    assert!(!status.eligible && status.grants.is_empty() && status.next_grant_id.is_none());
    assert!(
        wire.contains(r#""reset_grants":{"eligible":false,"grants":[]}"#),
        "{wire}"
    );
}
