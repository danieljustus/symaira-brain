use chrono::{DateTime, FixedOffset};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use symbrain_guard_core::external_decision::{ExternalDecisionAudit, evaluate_at};
use symbrain_guard_core::go_json::to_go_json_vec;

const FIXTURE: &[u8] = include_bytes!("fixtures/external_decision_review.json");
const SHA256: &str = "73057e5841975dafafd5c7ec40c43d459e766d6ca9567f277823569c76c8b09e";
const PIN: &str = "0b585d52915a824664e1377d0a995dff3f5405cd";

fn validate(bytes: &[u8]) -> Result<Value, String> {
    if format!("{:x}", Sha256::digest(bytes)) != SHA256 {
        return Err("reviewed Go capture hash mismatch".into());
    }
    let fixture: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if fixture["source_commit"] != PIN || fixture["schema_version"] != 1 {
        return Err("source pin/schema mismatch".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let generator = std::fs::read(root.join("guard/scripts/guard-decide-oracle/review_cases.py"))
        .map_err(|error| error.to_string())?;
    if fixture["generator_sha256"] != format!("{:x}", Sha256::digest(generator)) {
        return Err("generator changed".into());
    }
    let sources = fixture["source_files"]
        .as_object()
        .ok_or("source inventory")?;
    if sources.len() != 4 {
        return Err("source inventory count".into());
    }
    for (path, hash) in sources {
        let output = std::process::Command::new("git")
            .args(["show", &format!("{PIN}:{path}")])
            .current_dir(&root)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() || *hash != format!("{:x}", Sha256::digest(output.stdout)) {
            return Err(format!("historical source mismatch: {path}"));
        }
    }
    if fixture["cases"].as_array().ok_or("cases")?.len() != 35 {
        return Err("case inventory count".into());
    }
    Ok(fixture)
}

#[test]
fn reviewed_counterexamples_match_go_response_and_audit() {
    let fixture = validate(FIXTURE).expect("source-bound Go capture");
    let now: DateTime<FixedOffset> = "2026-09-14T12:00:00Z".parse().unwrap();
    let mut mismatches = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let mut audits = Vec::new();
        let mut sink = |record: &ExternalDecisionAudit| {
            audits.push(record.clone());
            Ok::<(), String>(())
        };
        let response = evaluate_at(case["input"].as_str().unwrap().as_bytes(), now, &mut sink);
        let mut bytes = to_go_json_vec(&response).unwrap();
        bytes.push(b'\n');
        let mut audit = serde_json::to_value(&audits[0]).unwrap();
        let fields = audit.as_object_mut().unwrap();
        fields.remove("id");
        fields.remove("decided_at");
        if bytes != case["stdout"].as_str().unwrap().as_bytes() || audit != case["audit_fields"] {
            mismatches.push(format!(
                "{}: response={response:?}; audit={audit}",
                case["id"]
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn mutated_review_capture_fails_the_actual_validator() {
    validate(FIXTURE).expect("valid capture");
    let mut fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    fixture["cases"][0]["stdout"] = Value::String("wrong-but-well-formed".into());
    assert_eq!(
        validate(&serde_json::to_vec(&fixture).unwrap()).unwrap_err(),
        "reviewed Go capture hash mismatch"
    );
}
