//! The `symbrain skills` shapes that used to be routed to the Go binary (#764):
//! with Go gone each one runs natively. These tests pin the intended Rust
//! behavior for configured libraries, cache overrides, other targets, scopes
//! and populated reports.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::fs;

use skills_sandbox::Sandbox;

fn custom_library(sandbox: &Sandbox) -> String {
    let library = sandbox.path("custom/library");
    fs::create_dir_all(&library).unwrap();
    for name in ["gamma", "delta"] {
        let dir = library.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} skill\ncategory: ops\n---\n# {name}\n"),
        )
        .unwrap();
    }
    library.to_str().unwrap().to_owned()
}

fn config_for(sandbox: &Sandbox, library: &str) {
    let base = sandbox.path("custom/base");
    sandbox.global_config(&format!(
        "library_dir = {library:?}\nbase_dir = {:?}\n",
        base.to_str().unwrap()
    ));
}

#[test]
fn configured_library_drives_list_status_and_sync_for_any_target() {
    let sandbox = Sandbox::new();
    let library = custom_library(&sandbox);
    config_for(&sandbox, &library);

    let listed = sandbox.skills(&["list", "--json"]).json();
    let names: Vec<&str> = listed["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["delta", "gamma"]);
    assert_eq!(listed["category_counts"]["ops"], 2);

    let installed = sandbox.skills(&["install", "--target", "claude", "--mode", "copy", "gamma"]);
    assert_eq!(installed.code, 0, "{}", installed.err);
    let status = sandbox.skills(&["status", "--target", "claude", "--json"]);
    assert_eq!(status.code, 0, "{}", status.err);
    assert_eq!(status.json()["installs"][0]["status"], "in-sync");

    fs::write(
        sandbox.path("custom/library/gamma/SKILL.md"),
        "---\nname: gamma\ndescription: gamma skill\ncategory: ops\n---\n# gamma changed\n",
    )
    .unwrap();
    let stale = sandbox
        .skills(&["status", "--target", "claude", "--json"])
        .json();
    assert_eq!(stale["installs"][0]["status"], "stale");
    assert_eq!(stale["summary"]["stale"], 1);

    let plan = sandbox.skills(&["sync", "--dry-run", "--json"]).json();
    assert_eq!(plan["dry_run"], true);
    assert_eq!(plan["results"][0]["name"], "gamma");
    assert!(
        fs::read_to_string(sandbox.path("home/.claude/skills/gamma/SKILL.md"))
            .unwrap()
            .contains("# gamma\n")
    );

    let repaired = sandbox.skills(&["sync", "--json"]);
    assert_eq!(repaired.code, 0, "{}", repaired.err);
    assert!(
        fs::read_to_string(sandbox.path("home/.claude/skills/gamma/SKILL.md"))
            .unwrap()
            .contains("changed")
    );
    let after = sandbox
        .skills(&["status", "--target", "claude", "--json"])
        .json();
    assert_eq!(after["installs"][0]["status"], "in-sync");
}

#[test]
fn project_config_overrides_the_global_config() {
    let sandbox = Sandbox::new();
    let library = custom_library(&sandbox);
    sandbox.global_config("library_dir = \"/nonexistent/global\"\n");
    fs::write(
        sandbox.path("project/.symskills.toml"),
        format!("library_dir = {library:?}\n"),
    )
    .unwrap();
    let listed = sandbox.skills(&["list", "--json"]);
    assert_eq!(listed.code, 0, "{}", listed.err);
    assert_eq!(listed.json()["skills"].as_array().unwrap().len(), 2);
}

#[test]
fn cache_dir_override_and_other_targets_report_without_render_extension() {
    let sandbox = Sandbox::new();
    let library = custom_library(&sandbox);
    config_for(&sandbox, &library);
    assert_eq!(
        sandbox
            .skills(&["install", "--target", "hermes", "--mode", "copy", "delta"])
            .code,
        0
    );
    let cache = sandbox.path("custom/cache");
    let cache = cache.to_str().unwrap();
    for target in ["hermes", "codex", "claude"] {
        let run = sandbox.skills_with(
            &[("SYMSKILLS_CACHE_DIR", cache)],
            &["status", "--target", target],
        );
        assert_eq!(run.code, 0, "{target}: {}", run.err);
        assert!(!run.out.contains("RENDER"), "{target}: {}", run.out);
    }
    let table = sandbox.skills_with(
        &[("SYMSKILLS_CACHE_DIR", cache)],
        &["status", "--target", "hermes"],
    );
    assert!(
        table.out.contains("hermes\tdelta\tin-sync\tcopy"),
        "{}",
        table.out
    );
    assert!(
        !sandbox.path("custom/cache").exists(),
        "status must not write the cache dir"
    );
}

#[test]
fn unknown_scope_and_target_are_usage_errors_and_targets_accepts_scope() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["status", "--scope", "galaxy"],
        vec!["status", "--target", "nowhere"],
        vec!["sync", "--scope", "galaxy"],
        vec!["targets", "--scope", "galaxy"],
    ] {
        let run = sandbox.skills(&args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.err);
        assert!(run.err.contains("unknown"), "{args:?}: {}", run.err);
    }
    for scope in ["user", "project"] {
        let run = sandbox.skills(&["targets", "--scope", scope, "--json"]);
        assert_eq!(run.code, 0, "{}", run.err);
        assert_eq!(run.json()["targets"].as_array().unwrap().len(), 6);
    }
}

#[test]
fn populated_list_reports_install_state_and_metadata() {
    let sandbox = Sandbox::new();
    let library = custom_library(&sandbox);
    config_for(&sandbox, &library);
    assert_eq!(
        sandbox
            .skills(&[
                "install",
                "--target",
                "claude,codex",
                "--mode",
                "copy",
                "gamma"
            ])
            .code,
        0
    );
    let listed = sandbox.skills(&["list", "--json"]).json();
    let gamma = listed["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "gamma")
        .unwrap()
        .clone();
    assert_eq!(gamma["category"], "ops");
    assert!(
        gamma["created_at"].is_string() && gamma["modified_at"].is_string(),
        "{gamma}"
    );
    let installs = gamma["installs"].as_array().unwrap();
    let targets: Vec<&str> = installs
        .iter()
        .map(|i| i["target"].as_str().unwrap())
        .collect();
    assert_eq!(targets, ["claude", "codex"]);
    let table = sandbox.skills(&["list"]);
    assert_eq!(table.code, 0, "{}", table.err);
    assert!(table.out.contains("gamma"), "{}", table.out);
}
