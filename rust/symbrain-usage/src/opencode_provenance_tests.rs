//! Source freshness is independent of replayed expectations. Regenerate only
//! with the pinned Go oracle; CI must never rewrite the accepted fixture.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "opencode_provenance_anchor.rs"]
mod anchor;

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
];
const HARNESS: &[&str] = &[
    "scripts/usage-request-oracle/opencode/main.go",
    "scripts/usage-request-oracle/opencode/cases.go",
    "scripts/usage-request-oracle/opencode/review_cases.go",
    "scripts/usage-request-oracle/opencode/provenance.go",
    "rust/symbrain-usage/src/opencode_discovery_tests.rs",
    "rust/symbrain-usage/src/opencode_provenance_tests.rs",
];
const CURRENT_FIXTURE: &str =
    "rust/symbrain-usage/tests/fixtures/opencode_discovery_go_dependency_update_20261008.json";
const HISTORICAL_FIXTURE: &str = "rust/symbrain-usage/tests/fixtures/opencode_discovery.json";

fn digest_text(data: &[u8]) -> String {
    let text = String::from_utf8(data.to_vec()).expect("provenance input must be UTF-8");
    format!(
        "{:x}",
        Sha256::digest(text.replace("\r\n", "\n").as_bytes())
    )
}

fn validate(oracle: &Value, root: &Path) {
    assert_eq!(oracle["schema_version"], 2);
    let p = &oracle["provenance"];
    assert_eq!(p["oracle_revision"], anchor::ORACLE_REVISION);
    assert_eq!(p["toolchain"], anchor::GO_TOOLCHAIN);
    for (key, paths) in [("source_hashes", SOURCES), ("harness_hashes", HARNESS)] {
        let map = p[key].as_object().expect("hash map required");
        assert_eq!(map.len(), paths.len(), "{key}: input inventory changed");
        for path in paths {
            let content = fs::read(root.join(path)).expect("manifested source required");
            let digest = digest_text(&content);
            assert_eq!(map[*path], digest, "{path}: rerun pinned Go oracle");
        }
    }
}

fn validate_anchor(root: &Path) {
    for &(path, expected) in anchor::EVIDENCE_HASHES {
        let actual = fs::read(root.join(path)).expect("anchored evidence file required");
        assert_eq!(digest_text(&actual), expected, "{path}: trust anchor drift");
    }
    let makefile = fs::read_to_string(root.join("Makefile")).expect("Makefile required");
    assert!(
        makefile.contains(&format!(
            "OPENCODE_ORACLE_REVISION := {}",
            anchor::ORACLE_REVISION
        )),
        "Makefile revision differs from the independent trust anchor"
    );
    assert!(
        makefile.contains(&format!(
            "OPENCODE_ORACLE_TOOLCHAIN := {}",
            anchor::GO_TOOLCHAIN
        )),
        "Makefile Go toolchain differs from the independent trust anchor"
    );
    assert!(
        makefile.contains("-oracle-revision $(OPENCODE_ORACLE_REVISION)")
            && makefile.contains("-oracle-toolchain $(OPENCODE_ORACLE_TOOLCHAIN)"),
        "native gate must pass anchored provenance explicitly to the generator"
    );
}

fn validate_current_fixture(data: &[u8], root: &Path) -> Value {
    assert_eq!(
        digest_text(data),
        anchor::CURRENT_FIXTURE_SHA256,
        "current fixture bytes differ from the independent trust anchor"
    );
    validate_anchor(root);
    let oracle: Value = serde_json::from_slice(data).expect("oracle fixture must parse");
    validate(&oracle, root);
    oracle
}

fn copy_validation_inputs(source: &Path, destination: &Path) {
    let paths: Vec<&str> = SOURCES
        .iter()
        .copied()
        .chain(HARNESS.iter().copied())
        .chain(anchor::EVIDENCE_HASHES.iter().map(|(path, _)| *path))
        .chain(std::iter::once("Makefile"))
        .collect();
    for path in paths {
        let to = destination.join(path);
        fs::create_dir_all(to.parent().expect("input has a parent")).unwrap();
        fs::copy(source.join(path), to).unwrap();
    }
}

fn temporary_root() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "opencode-provenance-{}-{unique}",
        std::process::id()
    ))
}

#[test]
fn provenance_checks_hashes_and_rejects_mutation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let current_bytes = fs::read(root.join(CURRENT_FIXTURE)).expect("current fixture required");
    let oracle = validate_current_fixture(&current_bytes, &root);

    let historical_bytes =
        fs::read(root.join(HISTORICAL_FIXTURE)).expect("historic fixture required");
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
    assert!(
        std::panic::catch_unwind(|| validate_current_fixture(&altered_case_bytes, &root)).is_err()
    );

    for key in ["source_hashes", "harness_hashes"] {
        let mut bad = oracle.clone();
        let hashes = bad["provenance"][key].as_object_mut().unwrap();
        *hashes.values_mut().next().unwrap() = Value::String("00".repeat(32));
        assert!(std::panic::catch_unwind(|| validate(&bad, &root)).is_err());
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
        assert!(std::panic::catch_unwind(|| validate(&bad, &root)).is_err());
    }

    let temporary = temporary_root();
    copy_validation_inputs(&root, &temporary);
    validate_anchor(&temporary);
    validate(&oracle, &temporary);
    for path in [SOURCES[0], HARNESS[3], HARNESS[5]] {
        let target = temporary.join(path);
        let original = fs::read(&target).unwrap();
        let mut changed = original.clone();
        changed.extend_from_slice(b"\nmutation");
        fs::write(&target, changed).unwrap();
        assert!(
            std::panic::catch_unwind(|| validate_current_fixture(&current_bytes, &temporary))
                .is_err(),
            "accepted mutated evidence file {path}"
        );
        fs::write(target, original).unwrap();
    }
    fs::remove_dir_all(temporary).unwrap();
}
