//! Maps the contract rows CLI-002..CLI-006, OUT-003, CFG-001, CFG-002 and
//! CFG-005 to the Go observations they are compared against, so a row cannot be
//! `parity` while its scenarios are missing from the frozen corpus.
//!
//! The comparison itself is `cli_oracle.rs::rust_cli_matches_frozen_go_cli_observations`
//! (every scenario below runs through the Rust binary and is compared on exit
//! code, stdout, stderr, daemon frames and written files). CLI-001 is the
//! frozen help tree replayed by `help_oracle.rs`. The corpus is reproducible:
//! `fixtures/cli/record_oracle.py` re-records it byte-identically from the Go
//! oracle binary named in `cli_oracle.provenance.json`.

use std::collections::BTreeSet;

use serde_json::Value;

const ORACLE: &str = include_str!("fixtures/cli/cli_oracle.json");
const PROVENANCE: &str = include_str!("fixtures/cli/cli_oracle.provenance.json");

/// (row, id prefixes, minimum scenario count)
const ROWS: &[(&str, &[&str], usize)] = &[
    (
        "CLI-001",
        &["help-", "root-", "completion-", "complete-"],
        10,
    ),
    ("CLI-002", &["alias-", "goto."], 6),
    ("CLI-003", &["args-", "unknown-", "bad-"], 40),
    ("CLI-004", &["flags-"], 50),
    ("CLI-005", &["eval-"], 40),
    ("CLI-006", &["batch-", "mcp-list-profiles"], 40),
    ("OUT-003", &["human-", "yaml-"], 240),
    ("CFG-001", &["cfgm-"], 250),
    ("CFG-002", &["xdg-", "home-", "socket-"], 60),
    ("CFG-005", &["engine-", "cfgm-engine"], 40),
];

fn scenario_ids() -> Vec<String> {
    let oracle: Value = serde_json::from_str(ORACLE).expect("oracle json");
    oracle["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_owned())
        .collect()
}

#[test]
fn every_owned_row_has_go_observations() {
    let ids = scenario_ids();
    for (row, prefixes, minimum) in ROWS {
        let count = ids
            .iter()
            .filter(|id| prefixes.iter().any(|p| id.starts_with(p)))
            .count();
        assert!(
            count >= *minimum,
            "{row}: only {count} recorded Go scenarios for {prefixes:?}, need {minimum}"
        );
    }
}

#[test]
fn corpus_ids_are_unique_and_match_the_provenance_count() {
    let ids = scenario_ids();
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate scenario ids");
    let provenance: Value = serde_json::from_str(PROVENANCE).expect("provenance json");
    assert_eq!(
        provenance["scenario_count"].as_u64(),
        Some(ids.len() as u64),
        "provenance scenario_count drifted from the corpus"
    );
    assert!(
        provenance["go_version"]
            .as_str()
            .is_some_and(|v| v.contains("go1.")),
        "provenance must name the oracle binary's Go toolchain"
    );
    assert_eq!(provenance["tool"], "record_oracle.py");
}
