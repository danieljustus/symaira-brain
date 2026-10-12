//! Actual frozen-Go/native status in both orders, including lock inventory.
use super::{compare_go, fixture_with_mode};
use std::fs;
use std::process::Command;

#[test]
fn frozen_go_and_native_status_release_render_locks_in_both_orders() {
    let oracle = std::env::var_os("SYMBRAIN_SKILLS_STATUS_GO_ORACLE");
    if std::env::var_os("SYMBRAIN_SKILLS_STATUS_REQUIRE_ORACLE").is_some() {
        assert!(oracle.is_some(), "live lock proof requires frozen Go");
    }
    let Some(oracle) = oracle else { return };
    let mut observations = Vec::new();
    for order in ["rust-go-rust", "go-rust-go"] {
        for target in symbrain_skills::default_targets() {
            let fixture = fixture_with_mode(&target, "copy");
            let mut native = fixture.command;
            native.args(["skills", "status", "--target", &target, "--json"]);
            let cache = fixture
                .temp
                .path()
                .join("cache/symbrain/skills/status-render");
            let mut steps = Vec::new();
            for producer in order.split('-') {
                let mut command = Command::new(if producer == "rust" {
                    std::ffi::OsStr::new(env!("CARGO_BIN_EXE_symbrain"))
                } else {
                    &oracle
                });
                command.env_clear().envs(
                    native
                        .get_envs()
                        .filter_map(|(key, value)| value.map(|value| (key, value))),
                );
                command
                    .current_dir(native.get_current_dir().unwrap())
                    .args(native.get_args());
                let output = command.output().expect("actual status process");
                assert!(
                    output.status.success(),
                    "{target}/{order}/{producer}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(output.stderr.is_empty(), "{target}/{order}/{producer}");
                let report: serde_json::Value =
                    serde_json::from_slice(&output.stdout).expect("complete report");
                assert_eq!(
                    report["installs"][0]["status"], "in-sync",
                    "{target}/{order}/{producer}"
                );
                let mut locks: Vec<_> = fs::read_dir(&cache)
                    .expect("comparison cache")
                    .map(|entry| {
                        entry
                            .expect("cache entry")
                            .file_name()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .filter(|name| name.starts_with(".symskills-lock-"))
                    .collect();
                locks.sort();
                if cfg!(windows) {
                    assert!(locks.is_empty(), "residual pathname: {locks:?}");
                }
                steps.push(serde_json::json!({"producer":producer,"report":report,"lock_paths":locks,"exit":output.status.code(),"stderr_empty":true}));
            }
            let rust = steps
                .iter()
                .find(|step| step["producer"] == "rust")
                .unwrap()["report"]
                .clone();
            let equality = compare_go(&oracle, &native, &target, order, &rust);
            observations.push(serde_json::json!({"target":target,"order":order,"steps":steps,"equality":equality}));
        }
    }
    assert_eq!(
        observations.len(),
        12,
        "complete six-target/order inventory"
    );
    if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_STATUS_EVIDENCE") {
        fs::write(std::path::Path::new(&path).with_extension("locks.json"), serde_json::to_vec_pretty(&serde_json::json!({"go_oracle_ref":"dcddcef0df5789123c7c9a7ebe6e01f10e941f2c","total":observations.len(),"cases":observations})).unwrap()).expect("lock evidence");
    }
}
