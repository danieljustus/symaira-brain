//! `symbrain skills doctor --catalog`: per-target budget, duplicate names and
//! lookalike descriptions through the real binary in a disposable HOME.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::fs;
use std::path::Path;

use skills_sandbox::Sandbox;

const NEAR: &str = "Use when reviewing pull requests for security flaws in rust code before merge";

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nbody\n"),
    )
    .unwrap();
}

fn rendered(sandbox: &Sandbox, target: &str, name: &str, description: &str) {
    write_skill(
        &sandbox
            .path("data/symbrain/skills/rendered")
            .join(target)
            .join(name),
        name,
        description,
    );
}

fn populated() -> Sandbox {
    let sandbox = Sandbox::new();
    for index in 0..9 {
        rendered(
            &sandbox,
            "claude",
            &format!("filler-{index}"),
            &format!("{index} {}", "d".repeat(999)),
        );
    }
    rendered(&sandbox, "claude", "review-a", NEAR);
    rendered(&sandbox, "claude", "review-b", &format!("{NEAR} today"));
    rendered(
        &sandbox,
        "claude",
        "budget",
        "Draft quarterly budget spreadsheets with currency conversion",
    );
    let user = sandbox.path("home/.claude/skills/shared");
    write_skill(&user, "shared", "user copy");
    fs::write(user.join(".symskills.json"), "{}").unwrap();
    write_skill(
        &sandbox.path("project/.claude/skills/shared"),
        "shared",
        "project copy",
    );
    sandbox
}

#[test]
fn default_doctor_has_no_catalog_section() {
    let sandbox = populated();
    let json = sandbox.skills(&["doctor", "--json"]);
    assert_eq!(json.code, 0, "{}", json.err);
    assert!(json.json().get("catalog").is_none(), "{}", json.out);
    let table = sandbox.skills(&["doctor"]);
    assert!(!table.out.contains("catalog"), "{}", table.out);
}

#[test]
fn catalog_json_reports_budget_duplicates_and_lookalikes_deterministically() {
    let sandbox = populated();
    let first = sandbox.skills(&["doctor", "--catalog", "--json"]);
    assert_eq!(first.code, 0, "{}", first.err);
    let second = sandbox.skills(&["doctor", "--catalog", "--json"]);
    assert_eq!(
        first.out, second.out,
        "doctor --json must be byte-identical"
    );

    let report = first.json();
    let catalog = &report["catalog"];
    assert_eq!(catalog["context_window_source"], "default");
    assert_eq!(catalog["context_window_tokens"], 200_000);
    let claude = catalog["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["target"] == "claude")
        .unwrap();
    assert_eq!(claude["skill_count"], 12);
    let chars = claude["description_chars"].as_u64().unwrap();
    assert_eq!(claude["listing_budget_chars"], 8000);
    assert_eq!(claude["over_budget"], true);
    assert_eq!(claude["over_by"].as_u64().unwrap(), chars - 8000);

    let duplicates = claude["duplicates"].as_array().unwrap();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0]["name"], "shared");
    let copies = duplicates[0]["copies"].as_array().unwrap();
    let seen: Vec<(&str, bool)> = copies
        .iter()
        .map(|copy| {
            (
                copy["scope"].as_str().unwrap(),
                copy["managed"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(seen, [("user", true), ("project", false)]);
    assert!(
        copies[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("home/.claude/skills/shared")
    );
    assert!(
        copies[1]["path"]
            .as_str()
            .unwrap()
            .ends_with("project/.claude/skills/shared")
    );

    let lookalikes = claude["lookalikes"].as_array().unwrap();
    assert_eq!(lookalikes.len(), 1);
    assert_eq!(
        (lookalikes[0]["a"].as_str(), lookalikes[0]["b"].as_str()),
        (Some("review-a"), Some("review-b"))
    );
}

#[test]
fn context_window_flag_changes_the_budget_and_implies_catalog() {
    let sandbox = populated();
    let run = sandbox.skills(&["doctor", "--context-window", "1000000", "--json"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let catalog = &run.json()["catalog"];
    assert_eq!(catalog["context_window_source"], "flag");
    let claude = catalog["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["target"] == "claude")
        .unwrap();
    assert_eq!(claude["listing_budget_chars"], 40_000);
    assert_eq!(claude["over_budget"], false);

    for bad in ["0", "abc"] {
        let run = sandbox.skills(&["doctor", "--context-window", bad]);
        assert_eq!(run.code, 2, "{bad}");
        assert!(run.err.contains("invalid --context-window"), "{}", run.err);
    }
}

#[test]
fn catalog_table_keeps_to_one_block_per_target() {
    let sandbox = populated();
    let run = sandbox.skills(&["doctor", "--catalog"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let out = run.out;
    assert!(
        out.contains("catalog     context window 200000 tokens (default)"),
        "{out}"
    );
    assert!(out.contains("claude      12 skills,"), "{out}");
    assert!(out.contains("budget 8000 chars, OVER by"), "{out}");
    assert!(out.contains("  duplicate   shared"), "{out}");
    assert!(out.contains("managed"), "{out}");
    assert!(out.contains("  lookalike   review-a ~ review-b"), "{out}");
    assert!(out.contains("antigravity "), "{out}");
    assert!(out.contains("budget not profiled"), "{out}");
}

#[test]
fn disabled_targets_are_absent_from_the_catalog() {
    let sandbox = populated();
    sandbox.global_config("disabled_targets = [\"claude\"]\n");
    let run = sandbox.skills(&["doctor", "--catalog", "--json"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let names: Vec<String> = run.json()["catalog"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["target"].as_str().unwrap().to_owned())
        .collect();
    assert!(!names.contains(&"claude".to_owned()), "{names:?}");
    assert_eq!(names.len(), 5);
}
