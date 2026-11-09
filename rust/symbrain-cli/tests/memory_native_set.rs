//! `symbrain memory set` against the Go oracle with the conflict checker,
//! PII redaction and fact extraction enabled (#758). The Rust process has no
//! Go fallback available.

#[path = "support/memory_native_oracle.rs"]
mod oracle;

use oracle::{Scenario, argv, compare};
use std::ffi::OsString;

const BIN: &str = env!("CARGO_BIN_EXE_symbrain");
const CONFIG: &str = "ollama.url = \"{EMBED}\"\n";

fn scenario(name: &str) -> Scenario<'_> {
    let mut scenario = Scenario::new(name, BIN);
    CONFIG.clone_into(&mut scenario.config);
    scenario
}

fn set(content: &str, extra: &[&str]) -> Vec<OsString> {
    let mut args = vec!["set", content, "--kind", "reference", "--db", "{DB}"];
    args.extend_from_slice(extra);
    argv(&args)
}

#[test]
fn plain_set_with_default_config() {
    let mut s = scenario("plain-default");
    s.steps = vec![set("hello world", &["--json"]), set("another note", &[])];
    compare(&s);
}

#[test]
fn set_without_pii_or_extraction_table_output() {
    let mut s = scenario("table");
    s.steps = vec![set("quarterly planning notes", &[])];
    compare(&s);
}

#[test]
fn set_redacts_pii_and_logs_the_patterns() {
    let mut s = scenario("pii");
    s.steps = vec![set("reach me at alice@example.com please", &["--json"])];
    compare(&s);
}

#[test]
fn set_extracts_secondary_facts_with_evidence() {
    let mut s = scenario("extract");
    s.steps = vec![set("I like tea. I use vim", &["--json"])];
    compare(&s);
}

#[test]
fn byte_identical_repeat_is_deduplicated() {
    let mut s = scenario("dedupe");
    s.seed = vec![set("the sky is blue today", &[])];
    s.steps = vec![set("the sky is blue today", &["--json"])];
    compare(&s);
}

#[test]
fn staged_write_bypasses_the_checker() {
    let mut s = scenario("staged");
    s.seed = vec![set("the sky is blue today", &[])];
    s.steps = vec![set("the sky is blue today", &["--staged", "--json"])];
    compare(&s);
}
