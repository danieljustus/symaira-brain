//! Globally disabled skill targets at the CLI boundary (#904).
//!
//! `disabled_targets = ["openclaw"]` in the symskills config switches a removed
//! harness off: it is listed as `disabled`, and neither status scans nor the
//! doctor report count it.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(config: Option<&str>) -> Self {
        let root = TempDir::new().unwrap();
        for name in ["home", "config/symskills", "data", "cache", "project"] {
            std::fs::create_dir_all(root.path().join(name)).unwrap();
        }
        for skills in [".claude/skills/demo", ".openclaw/skills/demo"] {
            std::fs::create_dir_all(root.path().join("home").join(skills)).unwrap();
        }
        if let Some(config) = config {
            std::fs::write(root.path().join("config/symskills/config.toml"), config).unwrap();
        }
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        let root: &Path = self.root.path();
        let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C.UTF-8")
            .current_dir(root.join("project"))
            .arg("skills")
            .args(args)
            .args(["--json"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        output
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.run(args).stdout).unwrap()
    }
}

fn state(report: &Value, target: &str) -> Option<String> {
    report["targets"]
        .as_array()?
        .iter()
        .find(|row| row["target"] == target)
        .map(|row| row["install_state"].as_str().unwrap().to_owned())
}

fn status_targets(report: &Value) -> Vec<String> {
    let mut targets: Vec<String> = report["installs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["target"].as_str().unwrap().to_owned())
        .collect();
    targets.dedup();
    targets
}

#[test]
fn targets_lists_a_disabled_target_as_disabled_without_scanning_it() {
    let fixture = Fixture::new(Some("disabled_targets = [\"openclaw\"]\n"));
    let report = fixture.json(&["targets"]);
    assert_eq!(state(&report, "openclaw").as_deref(), Some("disabled"));
    assert_ne!(state(&report, "claude").as_deref(), Some("disabled"));
    let row = report["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["target"] == "openclaw")
        .unwrap();
    assert_eq!(row["unmanaged_skills_count"], 0);
    assert_eq!(row["installed"], false);
}

#[test]
fn without_the_switch_every_target_is_scanned() {
    let fixture = Fixture::new(None);
    assert_ne!(
        state(&fixture.json(&["targets"]), "openclaw").as_deref(),
        Some("disabled")
    );
    assert_eq!(
        status_targets(&fixture.json(&["status"])),
        ["claude", "openclaw"]
    );
}

#[test]
fn status_skips_disabled_targets_unless_requested_explicitly() {
    let fixture = Fixture::new(Some("disabled_targets = [\"openclaw\"]\n"));
    assert_eq!(status_targets(&fixture.json(&["status"])), ["claude"]);
    assert_eq!(
        status_targets(&fixture.json(&["status", "--target", "openclaw"])),
        ["openclaw"]
    );
}

#[test]
fn project_config_adds_to_the_global_switch() {
    let fixture = Fixture::new(Some("disabled_targets = [\"codex\"]\n"));
    std::fs::write(
        fixture.root.path().join("project/.symskills.toml"),
        "disabled_targets = [\"openclaw\"]\n",
    )
    .unwrap();
    let report = fixture.json(&["targets"]);
    assert_eq!(state(&report, "codex").as_deref(), Some("disabled"));
    assert_eq!(state(&report, "openclaw").as_deref(), Some("disabled"));
}

#[test]
fn doctor_omits_disabled_targets() {
    let fixture = Fixture::new(Some("disabled_targets = [\"openclaw\"]\n"));
    let report = fixture.json(&["doctor"]);
    let names: Vec<&str> = report["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["target"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"claude"), "{names:?}");
    assert!(!names.contains(&"openclaw"), "{names:?}");
}
