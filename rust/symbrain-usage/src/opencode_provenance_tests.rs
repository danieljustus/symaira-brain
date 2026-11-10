//! The `OpenCode` oracle fixtures were recorded with the pinned Go oracle (see the provenance block
//! inside the fixture). The Go sources are gone, so this test no longer re-hashes them: the
//! recorded provenance, the independent byte anchors below and the sealed `scripts/oracle-freeze`
//! manifest are what bind the replayed expectations to the original recording.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[path = "opencode_provenance_anchor.rs"]
mod anchor;

const SOURCE_KEYS: &[&str] = &[
    "internal/usage/opencode.go",
    "internal/usage/opencode_parse.go",
    "internal/usage/provider.go",
    "internal/usage/schema.go",
    "internal/usage/http_security.go",
    "internal/usage/creds.go",
    "internal/usage/format.go",
    "internal/usage/testdata/opencode-workspaces.txt",
    "internal/usage/testdata/opencode-subscription-json.txt",
];
const HARNESS_KEYS: &[&str] = &[
    "scripts/usage-request-oracle/opencode/main.go",
    "scripts/usage-request-oracle/opencode/cases.go",
    "scripts/usage-request-oracle/opencode/review_cases.go",
    "scripts/usage-request-oracle/opencode/provenance.go",
    "rust/symbrain-usage/src/opencode_discovery_tests.rs",
    "rust/symbrain-usage/src/opencode_provenance_tests.rs",
];
const CURRENT_FIXTURE: &str =
    "tests/fixtures/opencode_discovery_go_dependency_update_20261008.json";
const HISTORICAL_FIXTURE: &str = "tests/fixtures/opencode_discovery.json";

fn digest_text(data: &[u8]) -> String {
    let text = String::from_utf8(data.to_vec()).expect("provenance input must be UTF-8");
    symbrain_core::hex::lower(Sha256::digest(text.replace("\r\n", "\n").as_bytes()))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn validate(oracle: &Value) {
    assert_eq!(oracle["schema_version"], 2);
    let p = &oracle["provenance"];
    assert_eq!(p["oracle_revision"], anchor::ORACLE_REVISION);
    assert_eq!(p["toolchain"], anchor::GO_TOOLCHAIN);
    for (key, paths) in [
        ("source_hashes", SOURCE_KEYS),
        ("harness_hashes", HARNESS_KEYS),
    ] {
        let map = p[key].as_object().expect("hash map required");
        assert_eq!(map.len(), paths.len(), "{key}: input inventory changed");
        for path in paths {
            let digest = map
                .get(*path)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{key}: {path} not recorded"));
            assert!(is_sha256_hex(digest), "{key}: {path} digest malformed");
        }
    }
}

fn validate_current_fixture(data: &[u8]) -> Value {
    assert_eq!(
        digest_text(data),
        anchor::CURRENT_FIXTURE_SHA256,
        "current fixture bytes differ from the independent trust anchor"
    );
    let oracle: Value = serde_json::from_slice(data).expect("oracle fixture must parse");
    validate(&oracle);
    oracle
}

#[test]
fn provenance_checks_hashes_and_rejects_mutation() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let current_bytes =
        fs::read(crate_root.join(CURRENT_FIXTURE)).expect("current fixture required");
    let oracle = validate_current_fixture(&current_bytes);

    let historical_bytes =
        fs::read(crate_root.join(HISTORICAL_FIXTURE)).expect("historic fixture required");
    assert_eq!(
        digest_text(&historical_bytes),
        anchor::HISTORICAL_FIXTURE_SHA256,
        "historic fixture was modified"
    );
    let historical: Value = serde_json::from_slice(&historical_bytes).unwrap();
    assert_eq!(
        oracle["cases"], historical["cases"],
        "dependency-only refresh changed the recorded behavior cases"
    );

    let mut altered_case_bytes = current_bytes.clone();
    let marker = b"missing workspace id";
    let position = altered_case_bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("expected response marker");
    altered_case_bytes[position] ^= 1;
    assert!(std::panic::catch_unwind(|| validate_current_fixture(&altered_case_bytes)).is_err());

    for key in ["source_hashes", "harness_hashes"] {
        let mut bad = oracle.clone();
        let hashes = bad["provenance"][key].as_object_mut().unwrap();
        *hashes.values_mut().next().unwrap() = Value::String("zz".repeat(32));
        assert!(std::panic::catch_unwind(|| validate(&bad)).is_err());
        let mut dropped = oracle.clone();
        let removed = dropped["provenance"][key]
            .as_object_mut()
            .unwrap()
            .keys()
            .next()
            .cloned()
            .unwrap();
        dropped["provenance"][key]
            .as_object_mut()
            .unwrap()
            .remove(&removed);
        assert!(std::panic::catch_unwind(|| validate(&dropped)).is_err());
    }
    for (key, value) in [
        (
            "oracle_revision",
            "0000000000000000000000000000000000000000",
        ),
        ("toolchain", "go0.0.0"),
    ] {
        let mut bad = oracle.clone();
        bad["provenance"][key] = Value::String(value.into());
        assert!(std::panic::catch_unwind(|| validate(&bad)).is_err());
    }
}
