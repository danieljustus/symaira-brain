//! Replays observations captured from the immutable production Go binary.
use serde_json::Value;
use sha2::{Digest, Sha256};
use symbrain_guard_core::external_decision::{ExternalDecisionAudit, evaluate_at};

#[test]
fn replay_f01_f05_production_go_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/external_repair_oracle.json")).unwrap();
    assert_eq!(
        fixture["oracle_commit"],
        "0b585d52915a824664e1377d0a995dff3f5405cd"
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 150);
    let generator = include_str!("../../../guard/scripts/guard-decide-oracle/repair_parity.py")
        .replace("\r\n", "\n");
    assert_eq!(
        fixture["generator_sha256"],
        format!("{:x}", Sha256::digest(generator.as_bytes()))
    );
    let now = "2026-09-15T00:00:00Z".parse().unwrap();
    for case in cases {
        let hex = case["input_hex"].as_str().unwrap();
        let input: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let mut records = Vec::new();
        let response = evaluate_at(&input, now, &mut |record: &ExternalDecisionAudit| {
            records.push(serde_json::to_value(record).unwrap());
            Ok(())
        });
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            case["response"],
            "{}",
            case["id"]
        );
        assert_eq!(records.len(), 1);
        let audit = records[0].as_object_mut().unwrap();
        audit.remove("id");
        audit.remove("decided_at");
        assert_eq!(records[0], case["audit"], "{}", case["id"]);
    }
}

#[test]
fn deadline_nanosecond_boundary_is_not_rounded() {
    let input =
        br#"{"command":"open","risk_class":"low","deadline":"2026-09-15T00:00:00.123456789Z"}"#;
    for (time, decision) in [
        ("2026-09-15T00:00:00.123456788Z", "allow"),
        ("2026-09-15T00:00:00.123456789Z", "deny"),
        ("2026-09-15T00:00:00.123456790Z", "deny"),
    ] {
        let response = evaluate_at(
            input,
            time.parse().unwrap(),
            &mut |_: &ExternalDecisionAudit| Ok(()),
        );
        assert_eq!(response.decision.to_string(), decision);
    }
}
