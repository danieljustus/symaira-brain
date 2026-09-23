//! Native sync root resolution over a legacy `symskills` layout (#621).
//!
//! Go resolves the skills data root through `internal/paths.resolve`:
//! `$XDG_DATA_HOME/symbrain/skills` when it exists, else the legacy
//! `$XDG_DATA_HOME/symskills` when only that exists (frozen as the `defaults`
//! scenarios in `symbrain-skills/tests/fixtures/runner_oracle.json`, which is
//! also the source of the expected roots below). The native CLI used to
//! resolve the current namespace unconditionally, so in the reporter's legacy
//! layout `sync` read an empty tree ("no skills rendered") and
//! `skills sync` claimed "Every installed skill is in sync." while Go
//! repaired six stale rows.
//!
//! Expected output bytes come from real `symbrain-go` runs captured for
//! issue #621 on 2026-09-22: the `skills sync` table (sha256
//! 52d7717f68aefcb455753a08b93171aa5fa0d201d7de7cd8d397940d2d1d4929) and the
//! per-target rendered/base `SKILL.md` hashes after Go repaired the edited
//! library.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

const SKILL_V1: &str = "---\nname: demo\ndescription: A demo\n---\n\n# Demo\nBody.\n";
const SKILL_V2: &str =
    "---\nname: demo\ndescription: A demo\n---\n\n# Demo\nBody.\n\n## New rule added in library\n";

/// Go's `skills sync` table for the repaired legacy layout, verbatim.
const GO_SYNC_TABLE: &str = "TARGET\tSKILL\tACTION\tDETAIL\n\
    antigravity\tdemo\tinstalled\t-\n\
    claude\tdemo\tinstalled\t-\n\
    codex\tdemo\tinstalled\t-\n\
    hermes\tdemo\tinstalled\t-\n\
    openclaw\tdemo\tinstalled\t-\n\
    opencode\tdemo\tinstalled\t-\n";

/// sha256 of each target's rendered `SKILL.md` after Go's repair — the same
/// bytes land in `<data root>/base/<target>/demo/SKILL.md`.
const GO_RENDERED_SHA: &[(&str, &str)] = &[
    (
        "antigravity",
        "713101088c9cc2748237aa2008859a467daf341c0831ec2e242b6b9a6402a2cf",
    ),
    (
        "claude",
        "06bcde45497b33cbe425f39bfd532a9568209629f7a2773fc8233520a7d387dc",
    ),
    (
        "codex",
        "f7bc7c157c8166402181d15968f6904cda969ddc71aa6f9f34926e1dbd5308b4",
    ),
    (
        "hermes",
        "c206999a901e82a63fc1ba059bd2638f7d9fa620bc73440955f637bf7fb1b668",
    ),
    (
        "openclaw",
        "4920e1f5c611278f4177a6df904dcbf9ee622f896cf4469cdc352deafabde98e",
    ),
    (
        "opencode",
        "c2cb37d61c126c50c7df6d429312b9a0704c9074abd9945c2ddb5e3f2364f50f",
    ),
];

/// One frozen `defaults` scenario from the Go runner oracle.
fn fixture_default(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../symbrain-skills/tests/fixtures/runner_oracle.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("oracle fixture")).unwrap();
    fixture
        .get("defaults")
        .and_then(serde_json::Value::as_array)
        .expect("fixture has defaults")
        .iter()
        .find(|case| case.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("fixture missing defaults scenario {name}"))
        .clone()
}

/// Resolves a fixture path. The fixture's `$ROOT` is the generator's global
/// temp root; every scenario lives at `$ROOT/<scenario name>/...`, so the
/// caller passes the temp root this test's scenario directory sits under.
fn fixture_path(case: &serde_json::Value, key: &str, root: &Path) -> PathBuf {
    let recorded = case
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("fixture case has no {key}"));
    PathBuf::from(recorded.replace("$ROOT", root.to_str().expect("root path is utf-8")))
}

/// Builds the isolated command environment under `scenario`.
fn command(scenario: &Path, xdg_data_home: Option<&Path>, args: &[&str]) -> Command {
    let home = scenario.join("home");
    let config = scenario.join("config");
    let cache = scenario.join("cache");
    let project = scenario.join("project");
    for path in [&home, &config, &cache, &project] {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(&project)
        .args(args);
    if let Some(data) = xdg_data_home {
        std::fs::create_dir_all(data).unwrap();
        command.env("XDG_DATA_HOME", data);
    }
    command
}

fn run(scenario: &Path, xdg_data_home: Option<&Path>, args: &[&str]) -> Output {
    command(scenario, xdg_data_home, args).output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is utf-8")
}

fn sha256_hex(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{error}"));
    format!("{:x}", Sha256::digest(bytes))
}

fn write_skill(path: &Path, body: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("SKILL.md"), body).unwrap();
}

/// Writes a skill whose directory and frontmatter names agree — the install
/// name comes from the frontmatter, so a mismatch would install under the
/// wrong directory name.
fn write_named_skill(dir: &Path, name: &str) {
    write_skill(
        dir,
        &format!("---\nname: {name}\ndescription: test\n---\n\n# {name}\nBody.\n"),
    );
}

/// The reporter's layout: only `$HOME/.local/share/symskills` exists and
/// `XDG_DATA_HOME` is unset (the oracle's `xdg_unset_uses_legacy` scenario).
/// Native bootstrap, root bytes, repair table and every repaired hash must
/// match what Go produced for the same scenario.
#[test]
fn legacy_layout_sync_repairs_like_go() {
    let tmp = TempDir::new().unwrap();
    let scenario = tmp.path().join("xdg_unset_uses_legacy");
    let case = fixture_default("xdg_unset_uses_legacy");
    assert_eq!(
        case.get("legacy_created"),
        Some(&serde_json::Value::Bool(true)),
        "the fixture must freeze the legacy-only branch"
    );
    assert_eq!(
        case.get("xdg_data_home"),
        Some(&serde_json::Value::String(String::new()))
    );

    // Rebuild exactly the filesystem state the fixture scenario describes.
    let legacy = scenario.join("home/.local/share/symskills");
    write_skill(&legacy.join("library/demo"), SKILL_V1);
    std::fs::create_dir_all(legacy.join("rendered")).unwrap();
    std::fs::create_dir_all(legacy.join("base")).unwrap();
    let expected_library = fixture_path(&case, "library_dir", tmp.path());
    let expected_render = fixture_path(&case, "render_dir", tmp.path());
    let expected_base = fixture_path(&case, "base_dir", tmp.path());
    assert_eq!(expected_library, legacy.join("library"));
    assert_eq!(expected_render, legacy.join("rendered"));
    assert_eq!(expected_base, legacy.join("base"));

    // 1. Native bootstrap reads the legacy library instead of an empty
    //    current namespace (pre-fix this reported "no skills rendered").
    let bootstrap = run(&scenario, None, &["sync"]);
    assert!(
        bootstrap.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    let bootstrap_stdout = stdout(&bootstrap);
    assert_eq!(
        bootstrap_stdout
            .matches("ok (1 skills rendered and installed)")
            .count(),
        6,
        "every skill target installs from the legacy library:\n{bootstrap_stdout}"
    );
    let hermes_link = scenario.join("home/.hermes/skills/symaira/demo");
    assert_eq!(
        std::fs::read_link(&hermes_link).unwrap(),
        expected_render.join("hermes/demo"),
        "installs link at the legacy render root Go resolves"
    );

    // 2. The resolved library root byte-matches the Go-frozen default: the
    //    populated native `skills list --json` reports paths under it.
    let list = run(&scenario, None, &["skills", "list", "--json"]);
    assert!(list.status.success(), "stderr: {:?}", list.stderr);
    let list_stdout = stdout(&list);
    assert!(
        list_stdout.contains(&format!("\"path\":\"{}/demo\"", expected_library.display())),
        "native list must resolve the legacy library root:\n{list_stdout}"
    );
    assert!(
        !list_stdout.contains("/symbrain/skills/"),
        "the current namespace must not be consulted:\n{list_stdout}"
    );

    // 3. Edit the library SSOT, then the native repair Go performs.
    std::fs::write(expected_library.join("demo/SKILL.md"), SKILL_V2).unwrap();
    let sync = run(&scenario, None, &["skills", "sync"]);
    assert!(
        sync.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(
        sync.stderr.is_empty(),
        "stderr: {:?}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert_eq!(
        sync.stdout,
        GO_SYNC_TABLE.as_bytes(),
        "skills sync must emit Go's exact repair table"
    );

    // 4. Every target's rendered and base snapshots advanced to Go's bytes,
    //    inside the legacy roots — and the install link still points there.
    for (target, sha) in GO_RENDERED_SHA {
        assert_eq!(
            sha256_hex(&expected_render.join(format!("{target}/demo/SKILL.md"))),
            *sha,
            "rendered/{target} must equal Go's repaired bytes"
        );
        assert_eq!(
            sha256_hex(&expected_base.join(format!("{target}/demo/SKILL.md"))),
            *sha,
            "base/{target} must equal Go's repaired bytes"
        );
    }
    assert_eq!(
        std::fs::read_link(&hermes_link).unwrap(),
        expected_render.join("hermes/demo"),
    );
    assert_eq!(
        sha256_hex(&hermes_link.join("SKILL.md")),
        GO_RENDERED_SHA[3].1,
        "installed skill resolves to Go's repaired bytes"
    );

    // 5. A second run converges, exactly like Go's.
    let again = run(&scenario, None, &["skills", "sync"]);
    assert!(again.status.success());
    assert_eq!(stdout(&again), "Every installed skill is in sync.\n");
}

/// When both namespaces exist the current one wins (the oracle's
/// `xdg_absolute_both_exist` scenario): native sync installs from
/// `symbrain/skills` and never touches the legacy tree.
#[test]
fn both_layouts_present_sync_uses_the_current_namespace() {
    let tmp = TempDir::new().unwrap();
    let scenario = tmp.path().join("xdg_absolute_both_exist");
    let case = fixture_default("xdg_absolute_both_exist");
    assert_eq!(
        case.get("current_created"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        case.get("legacy_created"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(case.get("legacy"), Some(&serde_json::Value::Bool(false)));

    let xdg_data = fixture_path(&case, "xdg_data_home", tmp.path());
    let current_library = fixture_path(&case, "library_dir", tmp.path());
    let legacy = xdg_data.join("symskills");
    write_named_skill(&current_library.join("current-skill"), "current-skill");
    write_named_skill(&legacy.join("library/legacy-skill"), "legacy-skill");
    std::fs::create_dir_all(legacy.join("rendered")).unwrap();
    std::fs::create_dir_all(legacy.join("base")).unwrap();

    let bootstrap = run(&scenario, Some(&xdg_data), &["sync"]);
    assert!(
        bootstrap.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    let bootstrap_stdout = stdout(&bootstrap);
    assert_eq!(
        bootstrap_stdout
            .matches("ok (1 skills rendered and installed)")
            .count(),
        6,
        "the current library drives every target:\n{bootstrap_stdout}"
    );
    assert!(
        scenario.join("home/.claude/skills/current-skill").exists(),
        "current-namespace skill installs"
    );
    assert!(
        !scenario.join("home/.claude/skills/legacy-skill").exists(),
        "the legacy library must not be read when the current one exists"
    );
    assert!(
        std::fs::read_dir(legacy.join("rendered"))
            .unwrap()
            .next()
            .is_none(),
        "the legacy tree stays untouched"
    );

    let list = run(&scenario, Some(&xdg_data), &["skills", "list", "--json"]);
    assert!(list.status.success(), "stderr: {:?}", list.stderr);
    let list_stdout = stdout(&list);
    assert!(
        list_stdout.contains(&format!(
            "\"path\":\"{}/current-skill\"",
            current_library.display()
        )),
        "native list resolves the current root:\n{list_stdout}"
    );
}

#[test]
fn native_sync_uses_the_platform_home_when_home_variables_disagree() {
    let tmp = TempDir::new().unwrap();
    let scenario = tmp.path().join("different-homes");
    let home = scenario.join("home");
    let other = scenario.join("other-home");
    let legacy = home.join(".local/share/symskills");
    write_named_skill(&legacy.join("library/demo"), "demo");

    let mut command = command(&scenario, None, &["sync"]);
    command.env(if cfg!(windows) { "HOME" } else { "USERPROFILE" }, &other);
    let output = command.output().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(
        stdout(&output)
            .matches("ok (1 skills rendered and installed)")
            .count(),
        6
    );
    assert_eq!(
        std::fs::read_link(home.join(".hermes/skills/symaira/demo")).unwrap(),
        legacy.join("rendered/hermes/demo"),
    );
    assert!(!other.join(".local/share/symskills").exists());
}
