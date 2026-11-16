//! `symbrain skills validate --targets`: opt-in advisory delivery findings
//! through the real binary; default `validate` output stays unchanged.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::fs;

use skills_sandbox::Sandbox;

fn skill(sandbox: &Sandbox, description: &str, body: &str, nested: bool) -> String {
    let dir = sandbox.path("src/probe");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: probe\ndescription: {description}\ncategory: dev\n---\n{body}\n"),
    )
    .unwrap();
    if nested {
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/SKILL.md"), "---\nname: sub\n---\nx\n").unwrap();
    }
    dir.to_str().unwrap().to_owned()
}

#[test]
fn default_validate_output_is_unchanged_by_the_feature() {
    let sandbox = Sandbox::new();
    let dir = skill(&sandbox, "no trigger here", &"x".repeat(120_000), true);
    for args in [
        &["validate", dir.as_str()][..],
        &["validate", "--json", dir.as_str()][..],
    ] {
        let run = sandbox.skills(args);
        assert_eq!(run.code, 0, "{}", run.err);
        assert!(!run.out.contains("harness_"), "{}", run.out);
        assert!(!run.out.contains("target_findings"), "{}", run.out);
    }
}

#[test]
fn targets_flag_reports_findings_in_table_and_json() {
    let sandbox = Sandbox::new();
    let dir = skill(&sandbox, "no trigger here", &"x".repeat(120_000), true);
    let table = sandbox.skills(&["validate", "--targets", &dir]);
    assert_eq!(table.code, 0, "{}", table.err);
    for code in [
        "harness_delivery_budget",
        "harness_listing_trigger",
        "harness_nested_manifest",
        "harness_read_budget",
    ] {
        assert!(
            table.out.contains(&format!("warning\t{code}\t")),
            "{code}: {}",
            table.out
        );
    }

    let json = sandbox.skills(&["validate", "--targets", "--json", &dir]);
    assert_eq!(json.code, 0, "{}", json.err);
    let value = json.json();
    let findings = value["target_findings"].as_array().unwrap();
    let nested_targets: Vec<&str> = findings
        .iter()
        .filter(|finding| finding["rule"] == "harness_nested_manifest")
        .map(|finding| finding["target"].as_str().unwrap())
        .collect();
    assert_eq!(nested_targets, ["opencode", "codex", "hermes"]);
    let budget = findings
        .iter()
        .find(|finding| finding["rule"] == "harness_delivery_budget")
        .unwrap();
    assert_eq!(budget["target"], "hermes");
    assert_eq!(budget["severity"], "warning");
    assert_eq!(budget["limit"], 100_000);
    assert_eq!(budget["harness_version"], "hermes 0.23.1");
    assert!(budget["measured"].as_u64().unwrap() > 100_000);
    assert_eq!(
        json.out,
        sandbox
            .skills(&["validate", "--targets", "--json", &dir])
            .out
    );
}

#[test]
fn strict_counts_target_findings_and_disabled_targets_are_silent() {
    let sandbox = Sandbox::new();
    let dir = skill(&sandbox, "Use when probing targets in tests", "tiny", false);
    let clean = sandbox.skills(&["validate", "--targets", "--strict", &dir]);
    assert_eq!(
        (clean.code, clean.out.as_str()),
        (0, "valid\n"),
        "{}",
        clean.err
    );

    let dir = skill(
        &sandbox,
        "Probes targets without any early trigger phrase",
        "tiny",
        false,
    );
    let strict = sandbox.skills(&["validate", "--targets", "--strict", &dir]);
    assert_eq!(strict.code, 1);
    assert!(
        strict.out.contains("harness_listing_trigger"),
        "{}",
        strict.out
    );

    sandbox.global_config("disabled_targets = [\"hermes\"]\n");
    let disabled = sandbox.skills(&["validate", "--targets", "--strict", &dir]);
    assert_eq!(
        (disabled.code, disabled.out.as_str()),
        (0, "valid\n"),
        "{}",
        disabled.err
    );
}
