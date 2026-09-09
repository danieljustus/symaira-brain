use std::collections::BTreeMap;
use std::fs;

use serde::Deserialize;
use symbrain_audit::{
    Config, Entry, Exposure, Logger, hash_entry, latest_degradations_in, tail_entries_in,
};
use tempfile::tempdir;

#[derive(Deserialize)]
struct Suite {
    entry_json: String,
    hashes: BTreeMap<String, String>,
    redactions: Vec<Redaction>,
    tail: TailCase,
    chained_tail_entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Redaction {
    id: String,
    server: String,
    tool: String,
    args: String,
    verbose: bool,
    arg_keys: String,
    arg_values: String,
}

#[derive(Deserialize)]
struct TailCase {
    files: BTreeMap<String, String>,
    profile: String,
    limit: usize,
    entries: Vec<Entry>,
    degradations: Vec<symbrain_audit::Degradation>,
}

fn suite() -> Suite {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse audit oracle")
}

#[test]
fn fixed_entry_json_and_chain_hashes_match_go() {
    let suite = suite();
    let entry: Entry = serde_json::from_str(&suite.entry_json).expect("entry");
    assert_eq!(
        serde_json::to_string(&entry).expect("serialize"),
        suite.entry_json
    );
    let first = hash_entry("hello", "");
    assert_eq!(first, suite.hashes["hello_genesis"]);
    assert_eq!(
        hash_entry("world", &first),
        suite.hashes["world_after_hello"]
    );
}

#[test]
fn logger_redaction_matches_go_oracle() {
    for case in suite().redactions {
        let dir = tempdir().expect("tempdir");
        let logger = Logger::open_in_with_session(
            dir.path(),
            &case.id,
            Config {
                enabled: true,
                verbose: case.verbose,
            },
            "fixed-session",
        )
        .expect("open");
        logger.log_at(
            "2026-01-01T00:00:00Z",
            &case.server,
            &case.tool,
            case.args.as_bytes(),
            1,
            "ok",
            &Exposure::default(),
            None,
        );
        logger.close().expect("close");
        let raw = fs::read_to_string(logger.path().expect("path")).expect("read");
        let envelope: serde_json::Value = serde_json::from_str(raw.trim()).expect("envelope");
        let entry: Entry =
            serde_json::from_str(envelope["d"].as_str().expect("payload")).expect("entry");
        assert_eq!(entry.arg_keys, case.arg_keys, "case {} keys", case.id);
        assert_eq!(entry.arg_values, case.arg_values, "case {} values", case.id);
    }
}

#[test]
fn bounded_tail_and_latest_degradations_match_go() {
    let case = suite().tail;
    let dir = tempdir().expect("tempdir");
    for (name, content) in case.files {
        fs::write(dir.path().join(name), content).expect("write fixture");
    }
    assert_eq!(
        tail_entries_in(dir.path(), &case.profile, case.limit).expect("tail"),
        case.entries
    );
    assert_eq!(
        latest_degradations_in(dir.path(), &case.profile).expect("degradations"),
        case.degradations
    );
}

#[test]
fn production_chained_tail_behavior_matches_frozen_go_defect() {
    let expected = suite().chained_tail_entries;
    let dir = tempdir().expect("tempdir");
    let logger = Logger::open_in_with_session(
        dir.path(),
        "chain",
        Config {
            enabled: true,
            verbose: false,
        },
        "fixed-session",
    )
    .expect("open");
    logger.log_at(
        "2026-01-01T00:00:00Z",
        "memory",
        "memory_search",
        br#"{"query":"term"}"#,
        1,
        "ok",
        &Exposure::default(),
        None,
    );
    logger.close().expect("close");
    assert_eq!(
        tail_entries_in(dir.path(), "chain", 1).expect("tail"),
        expected
    );
}
