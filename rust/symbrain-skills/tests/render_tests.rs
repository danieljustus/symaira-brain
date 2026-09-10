use std::fs;
use std::path::Path;

use symbrain_skills::{
    RenderMetadata, TargetConfig, default_targets, load_bundle, materialize, render_target,
};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata")
        .join(name)
}

#[test]
fn target_order_comes_from_canonical_harness_registry() {
    assert_eq!(
        default_targets(),
        [
            "claude",
            "opencode",
            "codex",
            "antigravity",
            "hermes",
            "openclaw"
        ]
    );
}

#[test]
fn unknown_and_disabled_targets_are_rejected_before_rendering() {
    let mut bundle = load_bundle(&fixture("source")).expect("source fixture");
    let unknown = render_target(&bundle, "not-a-target", &RenderMetadata::default())
        .expect_err("unknown target");
    let oracle: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
            .expect("oracle");
    let expected = oracle["security_cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == "unknown_target"))
        .and_then(|case| case["error"].as_str())
        .expect("unknown-target oracle case");
    assert_eq!(unknown.0, expected);

    bundle.manifest.targets.insert(
        "hermes".into(),
        TargetConfig {
            enabled: false,
            ..TargetConfig::default()
        },
    );
    let disabled =
        render_target(&bundle, "hermes", &RenderMetadata::default()).expect_err("disabled target");
    assert_eq!(disabled.0, "target hermes is disabled");
}

#[test]
fn opencode_source_matches_go_rendered_skill_bytes() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let expected = fs::read(fixture("golden/opencode/SKILL.md")).expect("golden SKILL.md");
    assert_eq!(rendered.skill_md, expected);
    assert_eq!(rendered.name, "golden-fixture-open");
}

#[test]
fn opencode_materialization_excludes_controls_and_preserves_support_bytes() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let materialized = materialize(&bundle, &rendered, output.path()).expect("materialize");
    assert_eq!(
        fs::read(materialized.root.join("scripts/helper.sh")).expect("script"),
        fs::read(fixture("golden/opencode/scripts/helper.sh")).expect("golden script")
    );
    let marker_path = materialized.root.join(".symskills.json");
    assert!(marker_path.is_file());
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker_path).expect("marker")).expect("marker");
    let oracle: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
            .expect("oracle");
    let expected_hash = oracle["render_cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == "source/opencode"))
        .and_then(|case| case["source_hash"].as_str())
        .expect("source hash oracle");
    assert_eq!(marker["source_hash"], expected_hash);
}

#[test]
fn opencode_variant_resolves_markdown_but_not_non_markdown() {
    let bundle = load_bundle(&fixture("variant-source")).expect("variant fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let skill = String::from_utf8(rendered.skill_md).expect("skill text");
    assert!(skill.contains("~/.local/state/symskills/reports/variant-fixture/"));
    assert!(!skill.contains("symskills:"));
    assert_eq!(rendered.files.len(), 1);
    assert!(rendered.files["references/execution-contract.md"].starts_with(b"# Execution"));
}

#[test]
fn capability_states_warn_unknown_and_force_unsupported() {
    let bundle = load_bundle(&fixture("oracle-edge")).expect("edge fixture");
    let mut metadata = RenderMetadata::default();
    let rendered = render_target(&bundle, "opencode", &metadata).expect("unknown renders");
    assert_eq!(
        rendered.warnings,
        vec![
            "target opencode has not declared capability \"subagents\" required by this skill; rendering anyway — record what your harness supports under [capabilities.opencode] in config.toml"
        ]
    );
    metadata.capabilities.insert("subagents".into(), false);
    assert!(render_target(&bundle, "opencode", &metadata).is_err());
    metadata.ignore_capabilities = true;
    let forced = render_target(&bundle, "opencode", &metadata).expect("forced render");
    assert!(forced.frontmatter.compatibility.is_empty());
    assert_eq!(forced.unmet_requirements[0].state, "unsupported");
}

#[test]
fn forced_render_clears_compatibility_after_overlay() {
    let root = tempfile::tempdir().expect("bundle root");
    fs::write(
        root.path().join("SKILL.md"),
        "---\nname: forced-overlay\ndescription: test\n---\nBody.\n",
    )
    .expect("SKILL.md");
    fs::write(
        root.path().join("symskills.toml"),
        "[skill]\nrequires = [\"subagents\"]\n",
    )
    .expect("manifest");
    let overlay = root.path().join("overlays/codex/frontmatter.toml");
    fs::create_dir_all(overlay.parent().expect("overlay parent")).expect("overlay parent");
    fs::write(&overlay, "compatibility = \"codex\"\n").expect("frontmatter overlay");
    let bundle = load_bundle(root.path()).expect("bundle");
    let mut metadata = RenderMetadata::default();
    metadata.capabilities.insert("subagents".into(), false);
    metadata.ignore_capabilities = true;
    let rendered = render_target(&bundle, "codex", &metadata).expect("forced render");
    assert!(rendered.frontmatter.compatibility.is_empty());
    assert!(!String::from_utf8_lossy(&rendered.skill_md).contains("compatibility:"));
}

#[test]
fn render_rejects_go_render_blocking_validation_cases() {
    let cases = [
        (
            "missing_configured_overlay",
            "---\nname: missing-overlay\ndescription: test\n---\nBody.\n",
            Some("[targets.opencode]\nenabled = true\nprepend = \"missing.md\"\n"),
            None,
        ),
        (
            "orphan_override",
            "---\nname: orphan-override\ndescription: test\n---\nBody.\n",
            None,
            Some(("overlays/opencode/blocks/invented.md", "replacement\n")),
        ),
        (
            "duplicate_block_id_across_files",
            "---\nname: duplicate-block\ndescription: test\n---\n<!-- symskills:block shared -->\nOne.\n<!-- /symskills:block -->\n",
            None,
            Some((
                "references/other.md",
                "<!-- symskills:block shared -->\nTwo.\n<!-- /symskills:block -->\n",
            )),
        ),
    ];
    for (name, skill_md, manifest, extra) in cases {
        let root = tempfile::tempdir().expect("bundle root");
        fs::write(root.path().join("SKILL.md"), skill_md).expect("SKILL.md");
        if let Some(manifest) = manifest {
            fs::write(root.path().join("symskills.toml"), manifest).expect("manifest");
        }
        if let Some((path, content)) = extra {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("parent");
            fs::write(path, content).expect("extra resource");
        }
        let bundle = load_bundle(root.path()).expect("bundle");
        let error = render_target(&bundle, "opencode", &RenderMetadata::default())
            .expect_err("render must reject blocking validation");
        assert!(error.0.starts_with("validation error: "), "{name}: {error}");
        match name {
            "missing_configured_overlay" => {
                assert!(error.0.contains("overlay reference \"missing.md\""));
            }
            "orphan_override" => assert!(error.0.contains("invented")),
            "duplicate_block_id_across_files" => assert!(error.0.contains("shared")),
            _ => unreachable!(),
        }
        let oracle: serde_json::Value =
            serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
                .expect("oracle");
        let expected = oracle["security_cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["id"] == name))
            .and_then(|case| case["error"].as_str())
            .expect("validation oracle case");
        assert_eq!(error.0, expected, "diagnostic drift for {name}");
    }
}

#[test]
fn materialize_merges_install_marker_metadata_with_deterministic_bytes() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let marker_root = output.path().join("opencode").join(&rendered.name);
    fs::create_dir_all(&marker_root).expect("marker root");
    fs::write(
        marker_root.join(".symskills.json"),
        br#"{"source_hash":"old","installed":true,"install_id":"abc"}"#,
    )
    .expect("old marker");

    let materialized = materialize(&bundle, &rendered, output.path()).expect("materialize");
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(materialized.root.join(".symskills.json")).expect("marker"),
    )
    .expect("marker");
    assert_eq!(marker["installed"], true);
    assert_eq!(marker["install_id"], "abc");
    assert_eq!(marker["source_hash"], materialized.source_hash);
    println!(
        "marker: {}",
        String::from_utf8_lossy(
            &fs::read(materialized.root.join(".symskills.json")).expect("marker")
        )
    );
    assert!(marker["output_digest"].as_str().is_some());
    assert!(marker["output_manifest"].as_array().is_some());
}

#[test]
fn materialize_replaces_malformed_and_non_object_markers() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    for invalid in [b"not-json".as_slice(), br#"["old"]"#] {
        let output = tempfile::tempdir().expect("output");
        let marker_root = output.path().join("opencode").join(&rendered.name);
        fs::create_dir_all(&marker_root).expect("marker root");
        fs::write(marker_root.join(".symskills.json"), invalid).expect("invalid marker");
        let materialized = materialize(&bundle, &rendered, output.path()).expect("materialize");
        let marker: serde_json::Value = serde_json::from_slice(
            &fs::read(materialized.root.join(".symskills.json")).expect("marker"),
        )
        .expect("rewritten marker");
        assert_eq!(marker["source_hash"], materialized.source_hash);
        assert!(marker["output_digest"].as_str().is_some());
        assert!(marker["output_manifest"].as_array().is_some());
    }
}

#[test]
fn materialization_failure_preserves_existing_tree() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let first = materialize(&bundle, &rendered, output.path()).expect("first materialize");
    fs::write(first.root.join("SKILL.md"), b"old skill\n").expect("old skill");
    let mut broken = rendered.clone();
    broken
        .files
        .insert("not-in-bundle.md".into(), b"poison\n".to_vec());
    let error = materialize(&bundle, &broken, output.path()).expect_err("broken render");
    assert!(error.0.contains("not-in-bundle.md"), "{error}");
    assert_eq!(
        fs::read(first.root.join("SKILL.md")).expect("preserved skill"),
        b"old skill\n"
    );
}
