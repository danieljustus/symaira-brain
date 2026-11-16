use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;
use crate::harness_profile::profile as profile_of;
use crate::load_bundle;

const GOOD: &str = "Use when checking harness delivery. Keeps findings quiet.";

fn rules(findings: &[Finding], target: &str) -> Vec<String> {
    findings
        .iter()
        .filter(|finding| finding.target == target)
        .map(|finding| finding.rule.clone())
        .collect()
}

type Case<'a> = (&'a str, String, &'a str, &'a Vec<String>, Vec<&'a str>);

#[test]
fn rules_fire_only_above_their_limit_table_driven() {
    let filler = |chars: usize| "x".repeat(chars);
    let nested = vec!["SKILL.md".to_owned(), "extra/SKILL.md".to_owned()];
    let plain = vec!["SKILL.md".to_owned(), "references/a.md".to_owned()];
    let late = format!("{} Use when late.", "a".repeat(60));
    // (target, skill_md, description, paths, expected rules)
    let cases: Vec<Case<'_>> = vec![
        // Two quote characters of the JSON string count against the budget.
        ("hermes", filler(99_998), GOOD, &plain, vec![]),
        (
            "hermes",
            filler(99_999),
            GOOD,
            &plain,
            vec![CODE_DELIVERY_BUDGET],
        ),
        (
            "hermes",
            "x".into(),
            &late,
            &plain,
            vec![CODE_LISTING_TRIGGER],
        ),
        (
            "hermes",
            "x".into(),
            "USE WHEN shouting in the first characters",
            &plain,
            vec![],
        ),
        (
            "hermes",
            "x".into(),
            GOOD,
            &nested,
            vec![CODE_NESTED_MANIFEST],
        ),
        ("codex", filler(40 * 1024), "no trigger", &plain, vec![]),
        (
            "codex",
            filler(40 * 1024 + 1),
            "no trigger",
            &plain,
            vec![CODE_READ_BUDGET],
        ),
        (
            "codex",
            "x".into(),
            "d",
            &nested,
            vec![CODE_NESTED_MANIFEST],
        ),
        (
            "opencode",
            "x".into(),
            "d",
            &nested,
            vec![CODE_NESTED_MANIFEST],
        ),
        ("opencode", filler(500_000), "d", &plain, vec![]),
        ("claude", filler(500_000), "d", &nested, vec![]),
        ("antigravity", filler(500_000), "d", &nested, vec![]),
        ("openclaw", filler(500_000), "d", &nested, vec![]),
    ];
    for (target, skill_md, description, paths, want) in cases {
        let profile = profile_of(target).unwrap();
        let facts = RenderedFacts {
            skill_md: &skill_md,
            description,
            paths,
        };
        let got: Vec<String> = evaluate_facts(profile, &facts)
            .into_iter()
            .map(|finding| finding.rule)
            .collect();
        assert_eq!(got, want, "{target} {description:?} {}", skill_md.len());
    }
}

#[test]
fn json_encoding_counts_escapes_not_source_chars() {
    for (text, want) in [("abc", 5), ("a\"b", 6), ("a\nb", 6), ("äö", 4), ("\\", 4)] {
        assert_eq!(json_encoded_chars(text), want, "{text:?}");
    }
    let hermes = profile_of("hermes").unwrap();
    // 60,000 newlines are 60,000 source chars but 120,000 JSON chars.
    let body = "\n".repeat(60_000);
    let facts = RenderedFacts {
        skill_md: &body,
        description: GOOD,
        paths: &[],
    };
    let findings = evaluate_facts(hermes, &facts);
    assert_eq!(rules(&findings, "hermes"), [CODE_DELIVERY_BUDGET]);
    assert_eq!(findings[0].measured, 120_002);
}

#[test]
fn finding_names_target_rule_measured_limit_and_version() {
    let hermes = profile_of("hermes").unwrap();
    let body = "x".repeat(100_001);
    let facts = RenderedFacts {
        skill_md: &body,
        description: GOOD,
        paths: &[],
    };
    let finding = &evaluate_facts(hermes, &facts)[0];
    assert_eq!(finding.target, "hermes");
    assert_eq!(finding.rule, CODE_DELIVERY_BUDGET);
    assert_eq!(finding.severity, "warning");
    assert_eq!((finding.measured, finding.limit), (100_003, 100_000));
    assert_eq!(finding.unit, "json_chars");
    assert_eq!(finding.harness_version, "hermes 0.23.1");
    assert_eq!(finding.verified_on, "2026-11-16");
    for needle in ["hermes 0.23.1", "100003", "100000", "1,500-char preview"] {
        assert!(
            finding.message.contains(needle),
            "{needle}: {}",
            finding.message
        );
    }
    let issue = finding.to_issue();
    assert_eq!(
        (issue.code.as_str(), issue.severity.as_str()),
        (CODE_DELIVERY_BUDGET, "warning")
    );
}

struct Skill {
    _temp: TempDir,
    root: PathBuf,
}

fn skill(description: &str, body: &str, extra: &[(&str, &str)]) -> Skill {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("fixture-skill");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: fixture-skill\ndescription: {description}\ncategory: test\n---\n{body}\n"
        ),
    )
    .unwrap();
    for (path, content) in extra {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    Skill { _temp: temp, root }
}

fn evaluate(skill: &Skill, disabled: &[&str]) -> Vec<Finding> {
    let disabled: Vec<String> = disabled.iter().map(|name| (*name).to_owned()).collect();
    evaluate_bundle(&load_bundle(&skill.root).unwrap(), &disabled)
}

#[test]
fn clean_fixture_reports_nothing_for_every_target() {
    assert_eq!(evaluate(&skill(GOOD, "Body", &[]), &[]), []);
}

#[test]
fn overlay_is_measured_on_the_rendered_bundle_only() {
    // The source body is tiny; only the hermes overlay pushes the render over.
    let big = "x".repeat(100_100);
    let fixture = skill(GOOD, "Body", &[("overlays/hermes/append.md", &big)]);
    let findings = evaluate(&fixture, &[]);
    assert_eq!(rules(&findings, "hermes"), [CODE_DELIVERY_BUDGET]);
    for other in ["codex", "opencode", "claude", "antigravity", "openclaw"] {
        assert!(rules(&findings, other).is_empty(), "{other}");
    }
}

#[test]
fn one_fixture_per_target_nested_manifest_and_read_budget() {
    let nested = skill(GOOD, "Body", &[("sub/SKILL.md", "---\nname: x\n---\nbody")]);
    let findings = evaluate(&nested, &[]);
    for target in ["hermes", "codex", "opencode"] {
        assert_eq!(rules(&findings, target), [CODE_NESTED_MANIFEST], "{target}");
    }
    for target in ["claude", "antigravity", "openclaw"] {
        assert!(rules(&findings, target).is_empty(), "{target}");
    }
    let nested_finding = findings.iter().find(|f| f.target == "codex").unwrap();
    assert_eq!((nested_finding.measured, nested_finding.limit), (1, 0));
    assert_eq!(nested_finding.path, "sub/SKILL.md");

    let large = skill(GOOD, &"y".repeat(41_000), &[]);
    let findings = evaluate(&large, &[]);
    assert_eq!(rules(&findings, "codex"), [CODE_READ_BUDGET]);
    assert!(rules(&findings, "hermes").is_empty());
    let under = skill(GOOD, &"y".repeat(30_000), &[]);
    assert_eq!(evaluate(&under, &[]), []);
}

#[test]
fn missing_trigger_reports_hermes_only_and_disabled_targets_are_silent() {
    let fixture = skill(
        "Checks things without any phrase near the start of text",
        "Body",
        &[],
    );
    let findings = evaluate(&fixture, &[]);
    assert_eq!(rules(&findings, "hermes"), [CODE_LISTING_TRIGGER]);
    assert_eq!(findings.len(), 1);

    assert_eq!(evaluate(&fixture, &["hermes"]), []);

    let manifest_off = skill(
        "No trigger here at all, just a long enough description to pass",
        "Body",
        &[("symskills.toml", "[targets.hermes]\nenabled = false\n")],
    );
    assert_eq!(evaluate(&manifest_off, &[]), []);
}
