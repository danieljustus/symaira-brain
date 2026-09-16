//! Replays observations captured from the immutable production Go binary.
use serde_json::Value;
use sha2::{Digest, Sha256};
use symbrain_guard_core::external_decision::{ExternalDecisionAudit, evaluate_at};

#[path = "fixtures/external_repair_trust_anchor.rs"]
mod trust_anchor;

const FIXTURE: &[u8] = include_bytes!("fixtures/external_repair_oracle.json");

#[test]
fn replay_f01_f05_production_go_observations() {
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE)),
        trust_anchor::TRUSTED_FIXTURE_SHA256
    );
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(
        fixture["oracle_commit"],
        trust_anchor::TRUSTED_ORACLE_COMMIT
    );
    assert_eq!(fixture["schema_version"], 2);
    assert_eq!(fixture["go_toolchain"], trust_anchor::TRUSTED_GO_TOOLCHAIN);
    let expected_source_files = trust_anchor::TRUSTED_SOURCE_FILES
        .iter()
        .map(|(path, hash)| ((*path).to_owned(), Value::String((*hash).to_owned())))
        .collect();
    assert_eq!(
        fixture["source_files"],
        Value::Object(expected_source_files)
    );
    assert_eq!(
        fixture["validation_basis"],
        serde_json::json!({
            "producer": "immutable Go binary",
            "argv": ["guard", "decide"],
            "compared": ["response", "audit"],
            "volatile_audit_fields": ["id", "decided_at"],
        })
    );
    let generator = include_str!("../../../guard/scripts/guard-decide-oracle/repair_parity.py")
        .replace("\r\n", "\n");
    let generator_sha256 = format!("{:x}", Sha256::digest(generator.as_bytes()));
    assert_eq!(generator_sha256, trust_anchor::TRUSTED_GENERATOR_SHA256);
    assert_eq!(fixture["generator_sha256"], generator_sha256);
    let validator = include_str!("../../../guard/scripts/guard-decide-oracle/native_repair.py")
        .replace("\r\n", "\n");
    let validator_sha256 = format!("{:x}", Sha256::digest(validator.as_bytes()));
    assert_eq!(validator_sha256, trust_anchor::TRUSTED_VALIDATOR_SHA256);
    assert_eq!(fixture["validation_basis_sha256"], validator_sha256);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), trust_anchor::TRUSTED_CASE_COUNT);
    assert_eq!(fixture["case_count"], trust_anchor::TRUSTED_CASE_COUNT);
    assert_eq!(fixture["case_ids"].as_array().unwrap().len(), cases.len());
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
