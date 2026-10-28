//! Current additive source freshness is independent of the frozen historical validator.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

fn validate_input(expected: &str, bytes: &[u8]) -> Result<(), &'static str> {
    if format!("{:x}", Sha256::digest(bytes)) == expected {
        Ok(())
    } else {
        Err("current captured source mismatch")
    }
}

#[test]
fn current_combined_source_matches_independent_capture_anchor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let anchor: Value = serde_json::from_slice(
        &std::fs::read(root.join("scripts/usage-contract-v2/accepted.json"))
            .expect("current additive contract requires a separately reviewed capture anchor"),
    )
    .unwrap();
    let inputs = anchor["inputs"]
        .as_object()
        .expect("captured source manifest");
    assert!(inputs.contains_key("internal/usage/schema.go"));
    assert!(inputs.contains_key("internal/usage/history.go"));
    assert!(inputs.contains_key("rust/symbrain-usage/src/history.rs"));
    for (path, hash) in inputs {
        let bytes = std::fs::read(root.join(path)).expect("captured input required");
        validate_input(hash.as_str().unwrap(), &bytes)
            .unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    for path in [
        "internal/usage/schema.go",
        "internal/usage/history.go",
        "rust/symbrain-usage/src/history.rs",
    ] {
        let original = std::fs::read(root.join(path)).unwrap();
        let hash = inputs[path].as_str().unwrap();
        assert!(validate_input(hash, &original).is_ok());
        let mut changed = original;
        changed.extend_from_slice(b"\nmutation");
        assert_eq!(
            validate_input(hash, &changed),
            Err("current captured source mismatch")
        );
    }
}
