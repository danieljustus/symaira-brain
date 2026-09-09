use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use symbrain_guard_core::{
    Result as PolicyResult,
    capability::{self, Error, Token},
};

#[derive(Deserialize)]
struct Suite {
    oracle_revision: String,
    source_sha256: BTreeMap<String, String>,
    generator_sha256: BTreeMap<String, String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    #[serde(default)]
    master_hex: String,
    #[serde(default)]
    token: String,
    claims: Option<Value>,
    scope: Option<Vec<String>>,
    #[serde(default)]
    target: String,
    result: Option<PolicyResult>,
    now: i64,
    #[serde(default)]
    output: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_kind: String,
}

fn suite() -> Suite {
    serde_json::from_slice(include_bytes!("fixtures/capability_oracle.json"))
        .expect("Go-generated capability fixture")
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").expect("write hex to string");
        out
    })
}

fn evaluate(case: &Case) -> Result<String, Error> {
    let master: Vec<u8> = case
        .master_hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let scope = case.scope.as_deref().unwrap_or_default();
    match case.kind.as_str() {
        "derive" => {
            capability::derive_key(&master).map(|key| serde_json::to_string(&hex(&key)).unwrap())
        }
        "sign" => {
            let json = serde_json::json!({"claims": case.claims.as_ref().unwrap()});
            let claims = Token::decode(&URL_SAFE_NO_PAD.encode(json.to_string()))?.claims;
            capability::sign(&master, claims)
                .map(|token| serde_json::to_string(&token.encode()).unwrap())
        }
        "decode" => {
            Token::decode(&case.token).map(|token| serde_json::to_string(&token.encode()).unwrap())
        }
        "verify" => capability::verify_at(&master, &case.token, case.now)
            .map(|claims| serde_json::to_string(&claims).unwrap()),
        "scope" => Ok(format!(
            "{{\"deny_control_plane\":{},\"in_scope\":{}}}",
            capability::deny_control_plane(&case.target),
            capability::in_scope(scope, &case.target)
        )),
        "ceiling" => Ok(
            serde_json::to_string(&symbrain_guard_core::scope::scope_ceiling(
                scope,
                &case.target,
                case.result.clone().unwrap(),
            ))
            .unwrap(),
        ),
        other => panic!("unknown oracle kind: {other}"),
    }
}

fn assert_case(case: &Case) {
    match evaluate(case) {
        Ok(actual) => {
            assert!(case.error.is_empty(), "{} unexpectedly accepted", case.id);
            // Verified claims and policy results compare declared JSON fields;
            // tokens, keys, and scope booleans compare exact Go-produced bytes.
            if matches!(case.kind.as_str(), "verify" | "ceiling") {
                assert_eq!(
                    serde_json::from_str::<Value>(&actual).unwrap(),
                    serde_json::from_str::<Value>(&case.output).unwrap(),
                    "{}",
                    case.id
                );
            } else {
                assert_eq!(actual, case.output, "{}", case.id);
            }
        }
        Err(error) => {
            assert_eq!(error.kind.as_str(), case.error_kind, "{}", case.id);
            assert_eq!(error.to_string(), case.error, "{}", case.id);
        }
    }
    println!("CAPABILITY_CASE_ID={}", case.id);
}

fn run_kinds(kinds: &[&str]) {
    let suite = suite();
    let cases: Vec<_> = suite
        .cases
        .iter()
        .filter(|c| kinds.contains(&c.kind.as_str()))
        .collect();
    assert!(!cases.is_empty(), "zero matched differential cases");
    for case in &cases {
        assert_case(case);
    }
    println!("CAPABILITY_EXECUTED={}", cases.len());
}

#[test]
fn capability_signing_matches_pinned_go_bytes() {
    run_kinds(&["derive", "sign"]);
}

#[test]
fn capability_verification_matches_pinned_go() {
    run_kinds(&["verify"]);
}

#[test]
fn capability_wire_matches_pinned_go_bytes() {
    run_kinds(&["decode"]);
}

#[test]
fn capability_scope_intersection_matches_pinned_go() {
    run_kinds(&["scope", "ceiling"]);
}

#[test]
fn capability_oracle_provenance_and_case_ids_are_complete() {
    use sha2::{Digest, Sha256};
    let suite = suite();
    assert_eq!(
        suite.oracle_revision,
        "0ccb0fe6c5afe668714d647eaa3497f8e6ed3f79"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(suite.source_sha256.len(), 4);
    assert_eq!(suite.generator_sha256.len(), 2);
    for (path, expected) in suite.source_sha256 {
        let bytes = std::fs::read(root.join(path)).unwrap();
        assert_eq!(hex(&Sha256::digest(bytes)), expected);
    }
    for (path, expected) in suite.generator_sha256 {
        let bytes = std::fs::read(root.join("guard/internal/capability").join(path)).unwrap();
        assert_eq!(hex(&Sha256::digest(bytes)), expected);
    }
    let ids: BTreeSet<_> = suite.cases.iter().map(|c| &c.id).collect();
    assert_eq!(ids.len(), suite.cases.len());
    let kinds: BTreeSet<_> = suite.cases.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(
        kinds,
        BTreeSet::from(["derive", "sign", "decode", "verify", "scope", "ceiling"])
    );
    assert!(ids.iter().all(|id| !id.is_empty()));
}
