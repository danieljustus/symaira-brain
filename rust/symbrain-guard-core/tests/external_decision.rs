use std::fs;
use std::path::PathBuf;

use base64::Engine;
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use symbrain_guard_core::external_decision::{
    AuditSink, ExternalDecision, ExternalDecisionAudit, evaluate_at,
};

#[derive(Debug, Clone, Deserialize)]
struct Fixture {
    schema_version: u8,
    source_commit: String,
    source_files: std::collections::BTreeMap<String, String>,
    cases_sha256: String,
    generator_sha256: String,
    cases: Vec<FixtureCase>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureCase {
    id: String,
    input_base64: String,
    exit_code: i32,
    stdout_base64: String,
    stderr_base64: String,
}

#[derive(Debug, Deserialize)]
struct HarnessDocument {
    cases: Vec<HarnessCase>,
}

#[derive(Debug, Deserialize)]
struct HarnessCase {
    id: String,
    request: Option<serde_json::Value>,
    raw: Option<String>,
    raw_prefix: Option<String>,
    raw_repeat: Option<String>,
    raw_repeat_count: Option<usize>,
    raw_suffix: Option<String>,
    boundary_note: Option<String>,
}

struct RecordingSink {
    records: Vec<ExternalDecisionAudit>,
    failure: Option<String>,
}

impl AuditSink for RecordingSink {
    fn write(&mut self, record: &ExternalDecisionAudit) -> Result<(), String> {
        self.records.push(record.clone());
        self.failure.clone().map_or(Ok(()), Err)
    }
}

fn fixture() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/external_decision.json");
    serde_json::from_str(&fs::read_to_string(path).expect("crate-local oracle fixture"))
        .expect("valid oracle fixture")
}

fn fixed_now() -> DateTime<FixedOffset> {
    "2026-09-14T12:00:00Z".parse().expect("fixed RFC3339 time")
}

const PINNED_COMMIT: &str = "0b585d52915a824664e1377d0a995dff3f5405cd";
const PINNED_SOURCE_FILES: [(&str, &str); 3] = [
    (
        "cmd/symbrain/cmd_guard.go",
        "cb0b10ae03a69b3c5c1d6952b706710a488ac4219fc908218c254056f2432abc",
    ),
    (
        "cmd/symbrain/main.go",
        "46a3245236f0104f34c7e0d0787fcb35c27a0eba6c6bba8d5a5098e1664c6e5b",
    ),
    (
        "guard/cmd/symguard/decide/command.go",
        "22fed158de555e04de72cb66fb63751c8812142e858df65ac0985a7cb450620f",
    ),
];

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

fn validate_fixture_provenance(fixture: &Fixture) -> Result<(), String> {
    if fixture.schema_version != 1 || fixture.source_commit != PINNED_COMMIT {
        return Err("fixture schema/source pin mismatch".into());
    }
    let expected_sources = PINNED_SOURCE_FILES
        .iter()
        .map(|(path, hash)| ((*path).to_owned(), (*hash).to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    if fixture.source_files != expected_sources {
        return Err("fixture source inventory/hash mismatch".into());
    }
    let root = repo_root();
    let cases_path = root.join("guard/scripts/guard-decide-oracle/cases.json");
    let generator_path = root.join("guard/scripts/guard-decide-oracle/oracle.py");
    let cases_bytes = fs::read(&cases_path).map_err(|e| format!("read cases: {e}"))?;
    let generator_bytes = fs::read(&generator_path).map_err(|e| format!("read generator: {e}"))?;
    if fixture.cases_sha256 != sha256_hex(&cases_bytes)
        || fixture.generator_sha256 != sha256_hex(&generator_bytes)
    {
        return Err("current harness provenance digest mismatch".into());
    }
    let harness: HarnessDocument =
        serde_json::from_slice(&cases_bytes).map_err(|e| format!("parse harness cases: {e}"))?;
    let acceptance = harness
        .cases
        .iter()
        .filter(|case| case.boundary_note.is_none())
        .collect::<Vec<_>>();
    if acceptance.len() != 33 || fixture.cases.len() != acceptance.len() {
        return Err("fixture/harness cardinality mismatch".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for (fixture_case, harness_case) in fixture.cases.iter().zip(acceptance) {
        if !ids.insert(fixture_case.id.clone()) || fixture_case.id != harness_case.id {
            return Err("fixture case IDs are not the ordered harness acceptance inventory".into());
        }
        let input = base64::engine::general_purpose::STANDARD
            .decode(&fixture_case.input_base64)
            .map_err(|e| format!("input base64: {e}"))?;
        if let Some(raw) = &harness_case.raw {
            if input != raw.as_bytes() {
                return Err(format!("input mismatch for {}", fixture_case.id));
            }
        } else if let Some(prefix) = &harness_case.raw_prefix {
            let repeat = harness_case.raw_repeat.as_deref().unwrap_or_default();
            let count = harness_case
                .raw_repeat_count
                .ok_or("missing repeat count")?;
            let suffix = harness_case
                .raw_suffix
                .as_deref()
                .ok_or("missing raw suffix")?;
            let expected = format!("{prefix}{}{suffix}", repeat.repeat(count));
            if input != expected.as_bytes() {
                return Err(format!("input mismatch for {}", fixture_case.id));
            }
        } else if let Some(request) = &harness_case.request {
            let actual: serde_json::Value = serde_json::from_slice(&input)
                .map_err(|e| format!("request binding for {}: {e}", fixture_case.id))?;
            if actual != *request {
                return Err(format!("request mismatch for {}", fixture_case.id));
            }
        } else {
            return Err(format!("harness case {} has no input", fixture_case.id));
        }
    }
    Ok(())
}

fn decoded(value: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .expect("fixture base64")
}

#[test]
fn pinned_go_fixture_matches_direct_response_bytes() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        fixture.source_commit,
        "0b585d52915a824664e1377d0a995dff3f5405cd"
    );
    validate_fixture_provenance(&fixture).expect("fixture provenance");

    let now = fixed_now();
    for case in &fixture.cases {
        assert_eq!(
            case.stderr_base64,
            base64::engine::general_purpose::STANDARD.encode([])
        );
        if case.id == "output-failure" {
            assert_eq!(case.exit_code, 1);
            assert!(decoded(&case.stdout_base64).is_empty());
            continue;
        }
        let failure = if case.id == "audit-failure" {
            let path =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audit-failure-data");
            Some(format!(
                "decide: mkdir audit dir: mkdir {}: not a directory",
                path.display()
            ))
        } else {
            None
        };
        let mut sink = RecordingSink {
            records: Vec::new(),
            failure,
        };
        let response = evaluate_at(&decoded(&case.input_base64), now, &mut sink);
        let mut actual = serde_json::to_vec(&response).expect("response JSON");
        actual.push(b'\n');
        let expected = decoded(&case.stdout_base64);
        let expected = if case.id == "audit-failure" {
            let path =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audit-failure-data");
            String::from_utf8(expected)
                .expect("sanitized response is UTF-8")
                .replace("__AUDIT_FAILURE_PATH__", &path.display().to_string())
                .into_bytes()
        } else {
            expected
        };
        assert_eq!(actual, expected, "{}", case.id);
        assert_eq!(case.exit_code, 0, "{}", case.id);
        assert_eq!(sink.records.len(), 1, "{} must be audited", case.id);
    }
}

#[test]
fn audit_failure_flips_allow_to_deny_but_keeps_existing_deny() {
    let now = fixed_now();
    let mut sink = RecordingSink {
        records: Vec::new(),
        failure: Some("injected audit failure".into()),
    };
    let response = evaluate_at(
        br#"{"command":"open","risk_class":"low","domain":"example.test","warnings":[]}"#,
        now,
        &mut sink,
    );
    assert_eq!(response.decision, ExternalDecision::Deny);
    assert_eq!(
        response.reason,
        "audit: write decision record: injected audit failure"
    );

    let response = evaluate_at(
        br#"{"command":"shell","risk_class":"high","warnings":["sudo"]}"#,
        now,
        &mut sink,
    );
    assert_eq!(response.decision, ExternalDecision::Deny);
    assert_eq!(response.reason, "high risk class with warnings: sudo");
}

#[test]
fn deadline_equality_is_expired_with_injected_time() {
    let now = fixed_now();
    let mut sink = RecordingSink {
        records: Vec::new(),
        failure: None,
    };
    let input = format!(
        r#"{{"command":"open","risk_class":"low","deadline":"{}"}}"#,
        now.to_rfc3339()
    );
    let response = evaluate_at(input.as_bytes(), now, &mut sink);
    assert_eq!(response.decision, ExternalDecision::Deny);
    assert_eq!(response.reason, "decide: request deadline expired");
}

#[test]
fn audit_record_preserves_received_fields_and_injected_timestamp() {
    let now = fixed_now();
    let mut records = Vec::new();
    let mut sink = |record: &ExternalDecisionAudit| {
        records.push(record.clone());
        Ok::<(), String>(())
    };
    let response = evaluate_at(br#"{"command":"open","risk_class":" LOW ","domain":" example.test ","warnings":[" popup "]}"#, now, &mut sink);
    assert_eq!(response.decision, ExternalDecision::Confirm);
    assert_eq!(records[0].risk_class, " LOW ");
    assert_eq!(records[0].warnings, Some(vec![" popup ".to_owned()]));
    assert_eq!(records[0].decided_at, "2026-09-14T12:00:00Z");
}

type FixtureMutation = Box<dyn Fn(&mut Fixture)>;

#[test]
fn fixture_provenance_mutations_are_rejected_by_the_same_validator() {
    let original = fixture();
    validate_fixture_provenance(&original).expect("positive provenance validation");
    let mutations: Vec<(&str, FixtureMutation)> = vec![
        (
            "source hash",
            Box::new(|f| {
                f.source_files
                    .values_mut()
                    .next()
                    .unwrap()
                    .replace_range(..64, &"0".repeat(64));
            }),
        ),
        (
            "generator hash",
            Box::new(|f| f.generator_sha256 = "0".repeat(64)),
        ),
        ("cases hash", Box::new(|f| f.cases_sha256 = "0".repeat(64))),
        (
            "removed case",
            Box::new(|f| {
                f.cases.pop();
            }),
        ),
        (
            "duplicate case",
            Box::new(|f| {
                let c = f.cases[0].clone();
                f.cases[1] = c;
            }),
        ),
        (
            "reordered case",
            Box::new(|f| {
                f.cases.swap(0, 1);
            }),
        ),
    ];
    for (name, mutate) in mutations {
        let mut altered = original.clone();
        mutate(&mut altered);
        assert!(
            validate_fixture_provenance(&altered).is_err(),
            "mutation must be rejected: {name}"
        );
    }
}

#[test]
fn trailing_json_diagnostics_match_go_byte_scanner() {
    let cases: &[(&[u8], &str)] = &[
        (br#"{"command":"open"}x"#, "'x'"),
        (b"{\"command\":\"open\"} \n\tx", "'x'"),
        (br#"{"command":"open"}{}"#, "'{'"),
        (br#"{"command":"open"}[]"#, "'['"),
        (b"{\"command\":\"open\"}\xc3\xa9", "'Ã'"),
        (b"{\"command\":\"open\"}\x07", "'\\a'"),
        (br#"{"command":"open"}""#, "'\"'"),
        (b"{\"command\":\"open\"}\\", "'\\\\'"),
    ];
    for (input, character) in cases {
        let mut sink = RecordingSink {
            records: Vec::new(),
            failure: None,
        };
        let response = evaluate_at(input, fixed_now(), &mut sink);
        assert_eq!(
            response.reason,
            format!("decide: parse request: invalid character {character} after top-level value"),
            "input: {input:?}"
        );
        assert_eq!(response.decision, ExternalDecision::Deny);
        assert_eq!(sink.records.len(), 1);
    }

    let mut sink = RecordingSink {
        records: Vec::new(),
        failure: None,
    };
    let response = evaluate_at(b"{\"command\":\"open\"} \n\t", fixed_now(), &mut sink);
    assert_eq!(response.decision, ExternalDecision::Deny);
    assert_eq!(response.reason, "decide: unknown risk class \"\"");
}
