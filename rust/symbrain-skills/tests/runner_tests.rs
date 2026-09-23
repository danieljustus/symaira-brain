//! Differential tests for the `symbrain-skills` runner against the frozen Go oracle.
//!
//! Each case reproduces a scenario from Go's
//! `internal/skillsrunner/runner_test.go` and asserts equality against
//! `tests/fixtures/runner_oracle.json`, which `scripts/skills-runner-oracle`
//! captures from real Go behaviour. The fixture, not Rust output, is the source
//! of truth.
//!
//! Scenarios that must set `HOME`, `XDG_DATA_HOME` or `PATH` live in
//! `runner_env_tests.rs`, where each one runs in a child process: this crate
//! denies `unsafe_code`, so a test cannot mutate its own environment.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use symbrain_skills::runner::{Context, NoopContext, Options};

/// Load the frozen Go oracle fixture.
fn load_fixture() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner_oracle.json");
    let raw = fs::read_to_string(&path).expect("fixture file must exist");
    serde_json::from_str(&raw).expect("fixture must be valid JSON")
}

/// Runner options rooted entirely in a throwaway directory.
fn options_for(root: &Path) -> Options {
    Options {
        library_dir: root.join("library").to_string_lossy().into(),
        render_dir: root.join("rendered").to_string_lossy().into(),
        base_dir: root.join("base").to_string_lossy().into(),
        home_dir: root.to_string_lossy().into(),
        ..Options::default()
    }
}

/// Writes one valid skill into a fresh library root and returns its path.
fn library_with_demo_skill(root: &Path) -> PathBuf {
    let library = root.join("library");
    let skill_dir = library.join("demo");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\n\n# Demo\nBody.\n",
    )
    .unwrap();
    library
}

/// Compares target/status/message triples against a fixture case.
fn assert_case_matches(
    case_name: &str,
    fixture: &serde_json::Value,
    results: &[symbrain_skills::runner::TargetResult],
) {
    let cases = fixture
        .get("cases")
        .expect("fixture has cases")
        .as_array()
        .unwrap();
    let expected_case = cases
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(case_name))
        .unwrap_or_else(|| panic!("fixture missing case: {case_name}"));

    let expected_results = expected_case
        .get("results")
        .expect("results")
        .as_array()
        .unwrap();
    assert_eq!(
        results.len(),
        expected_results.len(),
        "case {case_name} result count mismatch"
    );
    for (i, (got, exp_json)) in results.iter().zip(expected_results.iter()).enumerate() {
        let exp_target = exp_json.get("target").unwrap().as_str().unwrap();
        let exp_status = exp_json.get("status").unwrap().as_str().unwrap();
        let exp_message = exp_json
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        assert_eq!(got.target, exp_target, "{case_name}[{i}] target");
        assert_eq!(
            got.status, exp_status,
            "{case_name}[{i}] status (got msg={:?})",
            got.message
        );
        assert_eq!(
            got.message.as_deref().unwrap_or(""),
            exp_message,
            "{case_name}[{i}] message"
        );
    }
}

/// A timeout-capable context that fires after the given duration.
#[derive(Clone, Copy)]
struct TimeoutCtx {
    deadline: std::time::Instant,
}

impl TimeoutCtx {
    fn new(duration: Duration) -> Self {
        Self {
            deadline: std::time::Instant::now() + duration,
        }
    }
}

impl Context for TimeoutCtx {
    fn is_cancelled(&self) -> bool {
        std::time::Instant::now() >= self.deadline
    }
}

#[test]
fn fixture_records_every_go_scenario() {
    let f = load_fixture();
    let cases = f.get("cases").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 7, "fixture must record all Go test scenarios");
    let names: Vec<&str> = cases
        .iter()
        .map(|c| c.get("name").unwrap().as_str().unwrap())
        .collect();
    assert!(names.contains(&"empty_library_not_error"));
    assert!(names.contains(&"unsupported_harness_skipped"));
    assert!(names.contains(&"failure_visible_and_reported"));
    assert!(names.contains(&"no_legacy_binary_processes_targets"));
    assert!(names.contains(&"timeout_surfaces_error"));
    assert!(names.contains(&"binary_presence_changes_nothing"));
    assert!(names.contains(&"dry_run_failure_names_target"));
}

#[test]
fn empty_library_is_not_an_error() {
    let tmp = tempfile::tempdir().expect("tmp library dir");
    let library = tmp.path().join("library");
    fs::create_dir_all(&library).unwrap();

    let opts = options_for(tmp.path());
    let mut ctx = NoopContext;
    let res = symbrain_skills::runner::run(&mut ctx, &["claude".into()], opts, false)
        .expect("run should succeed for empty library");

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "ok");
    assert_eq!(
        res[0].message.as_deref().unwrap_or(""),
        "no skills rendered"
    );
    assert_eq!(res[0].target, "claude");

    let fixture = load_fixture();
    assert_case_matches("empty_library_not_error", &fixture, &res);
}

#[test]
fn unsupported_harness_yields_skipped_with_quoted_name() {
    let tmp = tempfile::tempdir().expect("tmp");
    let library = tmp.path().join("library");
    fs::create_dir_all(&library).unwrap();

    let opts = options_for(tmp.path());
    let mut ctx = NoopContext;
    let res = symbrain_skills::runner::run(
        &mut ctx,
        &["cursor".into(), "claude-desktop".into()],
        opts,
        false,
    )
    .expect("unsupported harnesses should not error");

    assert_eq!(res.len(), 2);
    // Go builds this message with %q on the harness name, so the quotes are part
    // of the contract: `no skill target for harness "cursor"`.
    assert_eq!(res[0].target, "cursor");
    assert_eq!(
        res[0].message.as_deref(),
        Some("no skill target for harness \"cursor\"")
    );
    assert_eq!(res[1].target, "claude-desktop");
    assert_eq!(
        res[1].message.as_deref(),
        Some("no skill target for harness \"claude-desktop\"")
    );

    let fixture = load_fixture();
    assert_case_matches("unsupported_harness_skipped", &fixture, &res);
}

#[test]
fn timeout_1ns_surfaces_deadline_exceeded() {
    // A minimal valid skill so the run enters sync_target, then times out. The
    // deadline comes from the option, exactly as Go's per-target WithTimeout.
    let tmp = tempfile::tempdir().expect("tmp");
    library_with_demo_skill(tmp.path());

    let opts = Options {
        timeout: Duration::from_nanos(1),
        ..options_for(tmp.path())
    };
    let mut ctx = TimeoutCtx::new(Duration::from_nanos(1));
    let res = symbrain_skills::runner::run(&mut ctx, &["claude".into()], opts, false)
        .expect("run completes, the error is embedded in the result");

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "error");
    assert_eq!(
        res[0].message.as_deref(),
        Some("sync timed out: context deadline exceeded")
    );

    let fixture = load_fixture();
    assert_case_matches("timeout_surfaces_error", &fixture, &res);
}

#[test]
fn broken_skill_names_the_target_in_the_install_path() {
    let tmp = tempfile::tempdir().expect("tmp");
    let library = tmp.path().join("library");
    let broken_dir = library.join("broken");
    fs::create_dir_all(&broken_dir).unwrap();
    fs::write(
        broken_dir.join("SKILL.md"),
        "---\nname: broken\ndescription: bad\n---\n\n<!-- symskills:blok typo -->\n# Bad\nBody.\n",
    )
    .unwrap();

    let opts = options_for(tmp.path());
    let mut ctx = NoopContext;
    let res = symbrain_skills::runner::run(&mut ctx, &["claude".into()], opts, false)
        .expect("a broken skill produces an error result, not a panic");

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "error");
    // Go's exact bytes, frozen in the fixture: the runner prepends
    // `<skill>: render: ` and the RenderAll path wraps `target <t>: `.
    let expected = "broken: render: target claude: validation error: line 6: \
                    unrecognised symskills marker: <!-- symskills:blok typo -->";
    assert_eq!(
        res[0].message.as_deref(),
        Some(expected),
        "broken-skill message must match Go byte for byte"
    );

    let fixture = load_fixture();
    assert_case_matches("failure_visible_and_reported", &fixture, &res);
}

// Runner defaults to managed symlink installs, unsupported by the Windows
// install layer; the copy-mode lifecycle is covered cross-platform elsewhere.
#[cfg(unix)]
#[test]
fn populated_library_processes_every_target_without_a_legacy_binary() {
    // Go's TestRun_NoLegacyBinaryProcessesTargets: no symskills binary is
    // consulted, and every harness with a skill target reports the installed
    // count. The fixture holds the four triples from the real Go run.
    let tmp = tempfile::tempdir().expect("tmp");
    library_with_demo_skill(tmp.path());

    let opts = options_for(tmp.path());
    let mut ctx = NoopContext;
    let res = symbrain_skills::runner::run(
        &mut ctx,
        &[
            "claude".into(),
            "codex".into(),
            "hermes".into(),
            "opencode".into(),
        ],
        opts,
        false,
    )
    .expect("run should succeed for a populated library");

    assert_eq!(res.len(), 4);
    for result in &res {
        assert_eq!(result.status, "ok", "target {}", result.target);
        assert_eq!(
            result.message.as_deref(),
            Some("1 skills rendered and installed"),
            "target {}",
            result.target
        );
    }

    let fixture = load_fixture();
    assert_case_matches("no_legacy_binary_processes_targets", &fixture, &res);
}
