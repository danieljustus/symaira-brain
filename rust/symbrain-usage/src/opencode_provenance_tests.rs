//! Source freshness is independent of replayed expectations. Regenerate only
//! with the pinned Go oracle; CI must never rewrite the accepted fixture.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

const SOURCES: &[&str] = &[
    "internal/usage/opencode.go",
    "internal/usage/opencode_parse.go",
    "internal/usage/provider.go",
    "internal/usage/schema.go",
    "internal/usage/http_security.go",
    "internal/usage/creds.go",
    "internal/usage/format.go",
    "internal/usage/testdata/opencode-workspaces.txt",
    "internal/usage/testdata/opencode-subscription-json.txt",
    "go.mod",
    "go.sum",
];
const HARNESS: &[&str] = &[
    "scripts/usage-request-oracle/opencode/main.go",
    "scripts/usage-request-oracle/opencode/cases.go",
    "scripts/usage-request-oracle/opencode/review_cases.go",
    "scripts/usage-request-oracle/opencode/provenance.go",
    "rust/symbrain-usage/src/opencode_discovery_tests.rs",
    "rust/symbrain-usage/src/opencode_provenance_tests.rs",
];

fn validate(oracle: &Value, root: &Path) {
    assert_eq!(oracle["schema_version"], 2);
    let p = &oracle["provenance"];
    assert_eq!(
        p["oracle_revision"],
        "a92385d2deecc08d1fd96869908b81b7abd355fe"
    );
    assert_eq!(p["toolchain"], "go1.26.7");
    for (key, paths) in [("source_hashes", SOURCES), ("harness_hashes", HARNESS)] {
        let map = p[key].as_object().expect("hash map required");
        assert_eq!(map.len(), paths.len(), "{key}: input inventory changed");
        for path in paths {
            let content =
                std::fs::read_to_string(root.join(path)).expect("manifested source required");
            let digest = format!(
                "{:x}",
                Sha256::digest(content.replace("\r\n", "\n").as_bytes())
            );
            assert_eq!(map[*path], digest, "{path}: rerun pinned Go oracle");
        }
    }
}

#[test]
fn provenance_checks_hashes_and_rejects_mutation() {
    let oracle: Value = serde_json::from_str(super::ORACLE).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    validate(&oracle, &root);
    for key in ["source_hashes", "harness_hashes"] {
        let mut bad = oracle.clone();
        let hashes = bad["provenance"][key].as_object_mut().unwrap();
        *hashes.values_mut().next().unwrap() = Value::String("00".repeat(32));
        assert!(std::panic::catch_unwind(|| validate(&bad, &root)).is_err());
    }
}
