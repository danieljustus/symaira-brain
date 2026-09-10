use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symbrain_skills::{
    MAX_BODY_LENGTH, MAX_DESCRIPTION_LENGTH, MAX_NAME_LENGTH, load_bundle, parse_skill_md,
    validate_skill_name, validate_with_targets,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata")
        .join(name)
}

#[test]
fn source_fixture_loads_frontmatter_manifest_and_resources() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    assert_eq!(bundle.frontmatter.name, "golden-fixture");
    assert_eq!(
        bundle.frontmatter.description,
        "A canonical fixture skill for per-target golden render tests"
    );
    assert_eq!(bundle.manifest.skill.version, "1.0.0");
    assert!(bundle.manifest.targets["opencode"].enabled);
    assert_eq!(
        bundle.manifest.targets["opencode"].alias,
        "golden-fixture-open"
    );
    assert_eq!(
        bundle.body,
        "# Golden Fixture\n\nThis fixture skill is used by per-target golden tests to assert that\nrendered skill bundles match each harness contract.\n\nIt exercises:\n- Frontmatter rendering with target-specific aliases\n- Support file copying (scripts directory)\n- Overlay application (opencode prepend)\n- Codex metadata generation (agents/openai.yaml)\n"
    );
    let resources = bundle
        .resources
        .iter()
        .map(|resource| resource.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        resources,
        [
            "overlays/opencode/prepend.md",
            "scripts/helper.sh",
            "symskills.toml"
        ]
    );
}

#[test]
fn variant_fixture_loads_markdown_and_overrides() {
    let bundle = load_bundle(&fixture("variant-source")).expect("variant fixture");
    assert_eq!(
        bundle.manifest.terms["report_dir"]["default"],
        "~/.local/state/symskills/reports"
    );
    assert_eq!(
        bundle.block_overrides["claude"]["worker-execution"],
        "Dispatch each group with the Agent tool, one subagent per group, each in its\nown git worktree under `<repo>/.worktrees/<task-slug>`.\n"
    );
    assert_eq!(
        bundle.block_overrides["hermes"]["dispatch"],
        "Use normal `delegate_task` children with the configured `strong` tier.\n"
    );
    assert!(
        bundle
            .markdown
            .contains_key("references/execution-contract.md")
    );
}

#[test]
fn frontmatter_normalizes_crlf_and_string_lists() {
    let parsed = parse_skill_md(b"---\r\nname: list-frontmatter\r\ndescription: [First, Second]\r\nauthor: [A, B]\r\nplatforms: [macOS, Linux]\r\n---\r\n\r\nBody.\r\n").expect("frontmatter");
    assert_eq!(parsed.frontmatter.description, "First, Second");
    assert_eq!(parsed.frontmatter.author, "A, B");
    assert_eq!(parsed.frontmatter.platforms, ["macOS", "Linux"]);
    assert_eq!(parsed.body, "Body.\n");
    assert_eq!(parsed.body_line_offset, 7);
}

#[test]
fn malformed_frontmatter_is_rejected_with_a_specific_error() {
    for input in [
        b"Just a body\n".as_slice(),
        b"---\nname: unclosed\ndescription: test\n".as_slice(),
        b"---\nname: [bad\ndescription: test\n---\nBody\n".as_slice(),
        b"---\nname: bad\ndescription:\n  nested: value\n---\nBody\n".as_slice(),
    ] {
        let error = parse_skill_md(input).expect_err("malformed frontmatter");
        assert!(error.to_string().contains("frontmatter"));
    }
}

#[test]
fn validation_matches_name_limits_and_render_blocking_policy() {
    for name in ["repo-review", "a", "my-skill-1"] {
        validate_skill_name(name).expect("valid skill name");
    }
    for name in [
        "",
        "Bad_Name",
        "a--b",
        "../escape",
        &"a".repeat(MAX_NAME_LENGTH + 1),
    ] {
        assert!(
            validate_skill_name(name).is_err(),
            "{name:?} should be invalid"
        );
    }
    assert!(symbrain_skills::is_render_blocking(
        "variant_marker_malformed"
    ));
    assert!(symbrain_skills::is_render_blocking(
        "overlay_reference_missing"
    ));
    assert!(!symbrain_skills::is_render_blocking("category_required"));
    assert_eq!(MAX_DESCRIPTION_LENGTH, 1024);
    assert_eq!(MAX_BODY_LENGTH, 50_000);
}

#[test]
fn validation_reports_source_marker_line_and_known_target_warnings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata/variant-source");
    let bundle = load_bundle(&root).expect("variant fixture");
    let known = [
        "claude",
        "hermes",
        "codex",
        "openclaw",
        "opencode",
        "antigravity",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    let issues = validate_with_targets(&bundle, &known);
    assert!(
        issues
            .iter()
            .all(|issue| issue.code != "variant_marker_malformed")
    );
    assert!(issues.iter().any(|issue| issue.code == "category_required"));
    assert!(issues.iter().all(|issue| !issue.message.is_empty()));
}

#[test]
fn validation_rejects_parent_traversal_overlay_reference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("safe-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(
        root.join("symskills.toml"),
        "[targets.hermes]\nenabled = true\nprepend = \"../SKILL.md\"\n",
    )
    .expect("manifest");
    let bundle = load_bundle(&root).expect("bundle");
    println!("targets: {:?}", bundle.manifest.targets);
    let issues = validate_with_targets(&bundle, &["hermes".into()]);
    assert!(issues.iter().any(|issue| {
        issue.code == "overlay_reference_missing" && issue.message.contains("escapes")
    }));
}

#[cfg(unix)]
#[test]
fn escaping_symlink_resource_is_rejected() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("safe-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "outside").expect("outside");
    symlink(outside, root.join("escape.txt")).expect("symlink");
    let error = load_bundle(&root).expect_err("escaping resource");
    assert!(error.to_string().contains("escapes skill root"));
}

#[test]
fn loader_matches_go_oracle_fixture() {
    let oracle_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle_expectations.json");
    let oracle: Value = serde_json::from_slice(&fs::read(oracle_path).expect("oracle fixture"))
        .expect("oracle JSON");
    for case in oracle["cases"].as_array().expect("oracle cases") {
        let id = case["id"].as_str().expect("case id");
        let bundle = load_bundle(&fixture(id)).expect("Go fixture bundle");
        let markdown_paths = bundle.markdown.keys().collect::<Vec<_>>();
        let target_names = bundle.manifest.targets.keys().collect::<Vec<_>>();
        let override_paths = bundle
            .block_overrides
            .iter()
            .map(|(target, blocks)| (target, blocks.keys().collect::<Vec<_>>()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let actual = json!({
            "id": id,
            "name": bundle.frontmatter.name,
            "description": bundle.frontmatter.description,
            "category": bundle.frontmatter.category,
            "version": bundle.frontmatter.version,
            "body": bundle.body,
            "resources": bundle.resources,
            "markdown_paths": markdown_paths,
            "override_paths": override_paths,
            "manifest_terms": bundle.manifest.terms,
            "target_names": target_names,
        });
        assert_eq!(actual, *case, "Go loader drift for {id}");
    }
}
