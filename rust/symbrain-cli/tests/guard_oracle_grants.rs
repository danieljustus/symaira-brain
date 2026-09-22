#![deny(unsafe_code)]

//! Byte-exact `symguard grants` CLI oracle (SEC-002): stdout and the
//! persisted grants.json bytes come from the real Go command via
//! `guard/scripts/guard-oracle`. The generator normalizes the store path
//! to `<store-dir>`, so the comparison is TMPDIR-independent. These cases
//! are owned here because the grants store and renderer live in
//! symbrain-cli, not in symbrain-guard-core.

use std::ffi::OsString;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(dead_code)]
#[path = "../src/guard_grants.rs"]
mod guard_grants;

const FIXTURE: &str =
    include_str!("../../symbrain-guard-core/tests/fixtures/oracle_expectations.json");

#[derive(Deserialize)]
struct Suite {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    input: Value,
    success: bool,
    #[serde(default)]
    output_json: String,
}

#[derive(Deserialize)]
struct GrantsCliInput {
    args: Vec<String>,
    store: String,
}

#[derive(Serialize)]
struct GrantsCliOutput {
    stdout: String,
    store_exists: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    store: String,
}

fn normalize(value: &str, dir: &Path) -> String {
    value.replace(&dir.display().to_string(), "<store-dir>")
}

#[test]
fn grants_cli_matches_go_oracle_bytes() {
    let suite: Suite = serde_json::from_str(FIXTURE).expect("fixture parses");
    let mut counted = 0_usize;
    for case in &suite.cases {
        if case.kind != "grants_cli" {
            continue;
        }
        counted += 1;
        assert!(
            case.success,
            "{}: grants_cli cases are success cases",
            case.id
        );
        let input: GrantsCliInput =
            serde_json::from_value(case.input.clone()).expect("grants_cli input");
        let dir = tempfile::tempdir().expect("temp dir");
        if !input.store.is_empty() {
            std::fs::write(dir.path().join("grants.json"), &input.store).expect("seed store");
        }
        let args: Vec<OsString> = input.args.iter().map(OsString::from).collect();
        let mut stdout = Vec::new();
        let code = guard_grants::run_at_dir(&args, dir.path(), &mut stdout);
        assert_eq!(code, 0, "{}: grants exit code", case.id);
        let stdout = String::from_utf8(stdout).expect("stdout utf8");
        let store_path = dir.path().join("grants.json");
        let persisted = std::fs::read_to_string(&store_path).ok();
        let output = GrantsCliOutput {
            stdout: normalize(&stdout, dir.path()),
            store_exists: persisted.is_some(),
            store: normalize(&persisted.unwrap_or_default(), dir.path()),
        };
        let bytes = symbrain_guard_core::go_json::to_go_json_vec(&output).expect("encode output");
        assert_eq!(
            String::from_utf8(bytes).expect("utf8 output"),
            case.output_json,
            "{}",
            case.id
        );
    }
    assert!(counted >= 1, "no grants_cli cases in the fixture");
}
