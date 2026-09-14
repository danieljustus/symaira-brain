use base64::Engine;
use serde::Deserialize;
use symbrain_guard_core::external_decision::{
    ExternalDecision, ExternalDecisionAudit, ExternalDecisionResponse,
};
use symbrain_guard_core::go_json::{format_go_rfc3339, to_go_json_vec};

#[test]
fn audit_serialization_omits_optional_empty_fields_like_go() {
    let record = ExternalDecisionAudit {
        id: "evt_1_decide_1".into(),
        command: "open".into(),
        risk_class: String::new(),
        domain: String::new(),
        warnings: None,
        decision: ExternalDecision::Deny,
        reason: String::new(),
        decided_at: "2026-09-14T12:00:00Z".into(),
    };
    let got = to_go_json_vec(&record).expect("audit JSON");
    assert_eq!(
        got,
        // The real pinned command capture in external_decision_review.json
        // verifies omitempty; the earlier mirror-only expectation did not.
        br#"{"id":"evt_1_decide_1","command":"open","decision":"deny","reason":"","decided_at":"2026-09-14T12:00:00Z"}"#
    );
}

#[test]
fn go_timestamp_is_utc_seconds_precision_for_audit_records() {
    let timestamp: chrono::DateTime<chrono::FixedOffset> =
        "2026-09-14T12:00:00.123456789+02:00".parse().unwrap();
    assert_eq!(format_go_rfc3339(timestamp), "2026-09-14T10:00:00Z");
}

#[derive(Deserialize)]
struct GoParityFixture {
    audit_base64: String,
    response_base64: String,
    timestamp: String,
}

#[test]
fn matches_pinned_go_generated_audit_and_response_fixture() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/go_json_parity.json");
    let fixture: GoParityFixture =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let timestamp: chrono::DateTime<chrono::FixedOffset> =
        "2026-09-14T12:00:00.123456789+02:00".parse().unwrap();
    assert_eq!(fixture.timestamp, format_go_rfc3339(timestamp));
    let record = ExternalDecisionAudit {
        id: "evt_1_decide_1".into(),
        command: "<&>\u{2028}\u{2029}".into(),
        risk_class: "low".into(),
        domain: "example.test".into(),
        warnings: Some(vec!["<&>\u{2028}\u{2029}".into()]),
        decision: ExternalDecision::Allow,
        reason: "<&>\u{2028}\u{2029}".into(),
        decided_at: fixture.timestamp.clone(),
    };
    let response = ExternalDecisionResponse {
        decision: ExternalDecision::Allow,
        reason: "<&>\u{2028}\u{2029}".into(),
    };
    assert_eq!(
        to_go_json_vec(&record).unwrap(),
        base64::engine::general_purpose::STANDARD
            .decode(fixture.audit_base64)
            .unwrap()
    );
    assert_eq!(
        to_go_json_vec(&response).unwrap(),
        base64::engine::general_purpose::STANDARD
            .decode(fixture.response_base64)
            .unwrap()
    );
}
