#![deny(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "../../symbrain-guard-core/tests/fixtures/external_repair_trust_anchor.rs"]
mod trust_anchor;

const FIXTURE: &str =
    include_str!("../../symbrain-guard-core/tests/fixtures/external_repair_oracle.json");
const RAW_CASE_IDS: [&str; 2] = ["raw-byte-top-level-80", "raw-byte-trailing-80"];

fn decode_hex(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2), "odd-length input hex");
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).expect("input hex"))
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_binary_matches_frozen_raw_byte_responses_and_audits() {
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE.as_bytes())),
        trust_anchor::TRUSTED_FIXTURE_SHA256
    );
    let fixture: Value = serde_json::from_str(FIXTURE).expect("raw-byte fixture JSON");
    assert_eq!(
        fixture["oracle_commit"],
        trust_anchor::TRUSTED_ORACLE_COMMIT
    );
    assert_eq!(fixture["go_toolchain"], trust_anchor::TRUSTED_GO_TOOLCHAIN);
    assert_eq!(fixture["case_count"], trust_anchor::TRUSTED_CASE_COUNT);
    let expected_source_files = trust_anchor::TRUSTED_SOURCE_FILES
        .iter()
        .map(|(path, hash)| ((*path).to_owned(), Value::String((*hash).to_owned())))
        .collect();
    assert_eq!(
        fixture["source_files"],
        Value::Object(expected_source_files)
    );
    let generator = include_str!("../../../guard/scripts/guard-decide-oracle/repair_parity.py")
        .replace("\r\n", "\n");
    assert_eq!(
        format!("{:x}", Sha256::digest(generator.as_bytes())),
        trust_anchor::TRUSTED_GENERATOR_SHA256
    );
    let validator = include_str!("../../../guard/scripts/guard-decide-oracle/native_repair.py")
        .replace("\r\n", "\n");
    assert_eq!(
        format!("{:x}", Sha256::digest(validator.as_bytes())),
        trust_anchor::TRUSTED_VALIDATOR_SHA256
    );
    let cases = fixture["cases"].as_array().expect("fixture cases");

    let raw_cases: Vec<&Value> = cases
        .iter()
        .filter(|case| {
            case["id"]
                .as_str()
                .is_some_and(|id| RAW_CASE_IDS.contains(&id))
        })
        .collect();
    assert_eq!(raw_cases.len(), RAW_CASE_IDS.len());

    for id in RAW_CASE_IDS {
        let case = raw_cases
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("missing pinned case {id}"));
        let input = decode_hex(case["input_hex"].as_str().expect("input hex"));
        let runtime = tempfile::tempdir().expect("runtime tempdir");
        let data_home = runtime.path().join("data");
        let home = runtime.path().join("home");
        let config = runtime.path().join("config");
        let cache = runtime.path().join("cache");
        let temp = runtime.path().join("tmp");

        let mut child = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .args(["guard", "decide"])
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_home)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache)
            .env("TMPDIR", &temp)
            .env("TMP", &temp)
            .env("TEMP", &temp)
            .env("TZ", "UTC")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn production symbrain binary");
        child
            .stdin
            .take()
            .expect("production stdin")
            .write_all(&input)
            .expect("write raw-byte request");
        let output = child
            .wait_with_output()
            .expect("wait for production symbrain binary");

        assert!(output.status.success(), "{id}: {:#?}", output.status);
        assert!(output.stderr.is_empty(), "{id}: stderr is not empty");
        let expected_response = &case["response"];
        let mut expected_stdout = serde_json::to_vec(expected_response).expect("response JSON");
        expected_stdout.push(b'\n');
        assert_eq!(output.stdout, expected_stdout, "{id}: response bytes");

        let audit_path = data_home.join("symguard/audit.log");
        let audit_bytes = std::fs::read(&audit_path).unwrap_or_else(|error| {
            panic!(
                "{id}: read persisted audit {}: {error}",
                audit_path.display()
            )
        });
        let mut lines = audit_bytes.split(|byte| *byte == b'\n');
        let line = lines.next().expect("one persisted audit record");
        assert!(!line.is_empty(), "{id}: empty audit record");
        assert_eq!(
            lines.next(),
            Some(&[][..]),
            "{id}: audit is not one JSONL record"
        );
        assert!(lines.next().is_none(), "{id}: more than one audit record");
        let mut audit = serde_json::from_slice::<Value>(line).expect("audit JSON");
        let object = audit.as_object_mut().expect("audit object");
        assert!(object.remove("id").is_some(), "{id}: audit id");
        assert!(
            object.remove("decided_at").is_some(),
            "{id}: audit timestamp"
        );
        assert_eq!(audit, case["audit"], "{id}: persisted audit fields");
    }
}
