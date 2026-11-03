//! Help text of every Go cobra command against the frozen tree
//! (`testdata/port/cli/tree.json`, recorded from the real Go binary by
//! `testdata/port/cli/gen_tree.py`; provenance is inside the file). Completion
//! scripts and `__complete` answers are replayed by `cli_oracle.rs`. No Go
//! binary, source or toolchain is needed at test time.

use std::{path::Path, process::Command};

use serde_json::Value;

const TREE: &str = include_str!("../../../testdata/port/cli/tree.json");
const RETIRED: &str = "compat-sidecar";

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(Path::new(env!("CARGO_BIN_EXE_symbrowse")))
        .args(args)
        .env_clear()
        .env("LANG", "C")
        .env("HOME", std::env::temp_dir())
        .output()
        .expect("run symbrowse");
    (
        output.status.code().expect("exit code"),
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        String::from_utf8(output.stderr).expect("UTF-8 stderr"),
    )
}

/// Go lists the retired `compat-sidecar` command; Rust must not (the column
/// width is set by the longer `scrollintoview`, so dropping the line is exact).
fn without_retired(help: &str) -> String {
    help.split_inclusive('\n')
        .filter(|line| !line.starts_with(&format!("  {RETIRED} ")))
        .collect()
}

#[test]
fn frozen_tree_has_go_provenance() {
    let tree: Value = serde_json::from_str(TREE).expect("tree json");
    let provenance = &tree["provenance"];
    assert!(
        provenance["go_version"]
            .as_str()
            .unwrap()
            .starts_with("go version go1.")
    );
    assert!(provenance["go_binary_sha256"].as_str().unwrap().len() == 64);
    assert!(!tree["nodes"].as_array().unwrap().is_empty());
}

#[test]
fn help_for_every_go_command_matches_the_frozen_tree() {
    let tree: Value = serde_json::from_str(TREE).expect("tree json");
    let mut checked = 0;
    for node in tree["nodes"].as_array().expect("nodes") {
        let path: Vec<&str> = node["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("part"))
            .collect();
        if path.first() == Some(&RETIRED) {
            let (code, _, _) = run(&[RETIRED, "--help"]);
            assert_eq!(
                code, 2,
                "retired command must be a typed transport_retired/unknown error"
            );
            continue;
        }
        let expected = without_retired(node["help"].as_str().expect("help"));
        let mut args = path.clone();
        args.push("--help");
        let (code, stdout, stderr) = run(&args);
        assert_eq!((code, stderr.as_str()), (0, ""), "{path:?} --help");
        assert_eq!(stdout, expected, "help drift for {path:?}");
        if !path.is_empty() {
            let mut via_help = vec!["help"];
            via_help.extend(&path);
            let (code, stdout, stderr) = run(&via_help);
            assert_eq!((code, stderr.as_str()), (0, ""), "{via_help:?}");
            assert_eq!(stdout, expected, "`help` drift for {path:?}");
        }
        checked += 1;
    }
    assert!(checked > 100, "only {checked} commands checked");
}
