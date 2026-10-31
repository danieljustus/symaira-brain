//! Dedicated additive-contract proof; frozen `OpenCode` captures and freshness gate remain unchanged.
use sha2::{Digest, Sha256};

fn legacy_projection(source: &str) -> Option<String> {
    let addition = "\tHistory     *LocalHistory `json:\"history,omitempty\"`\n\tID          string        `json:\"id\"`\n\tDisplayName string        `json:\"display_name\"`\n\tConfigured  bool          `json:\"configured\"`\n\tAuthStatus  AuthStatus    `json:\"auth_status\"`";
    let legacy = "\tID          string     `json:\"id\"`\n\tDisplayName string     `json:\"display_name\"`\n\tConfigured  bool       `json:\"configured\"`\n\tAuthStatus  AuthStatus `json:\"auth_status\"`";
    (source.matches(addition).count() == 1).then(|| source.replacen(addition, legacy, 1))
}
fn legacy_matches(source: &str) -> bool {
    legacy_projection(source).is_some_and(|s| {
        format!("{:x}", Sha256::digest(s.as_bytes()))
            == "edd8a646e954078b25e7b22677d778885755019e94dd98267304665084df4f2a"
    })
}
#[test]
fn additive_history_preserves_exact_frozen_go_schema_projection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = std::fs::read_to_string(root.join("internal/usage/schema.go"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(legacy_matches(&source));
    assert!(!legacy_matches(
        &source.replace("history,omitempty", "history")
    ));
    assert!(!legacy_matches(
        &source.replace("json:\"used,omitempty\"", "json:\"used\"")
    ));
    assert!(!legacy_matches(&(source + "\nmutation")));
}
