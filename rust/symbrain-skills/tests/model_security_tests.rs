use std::fs;
use std::path::Path;
use symbrain_skills::{MAX_RESOURCE_SIZE, load_bundle};

#[path = "common/mod.rs"]
mod common;

fn oracle_bundle(name: &str) -> serde_json::Value {
    let oracle: serde_json::Value =
        serde_json::from_slice(&common::skills_oracle()).expect("Go skills oracle");
    oracle["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == name))
        .unwrap_or_else(|| panic!("missing Go oracle bundle {name}"))
        .clone()
}

fn oracle_security_case(name: &str) -> serde_json::Value {
    let oracle: serde_json::Value =
        serde_json::from_slice(&common::skills_oracle()).expect("Go skills oracle");
    oracle["security_cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == name))
        .unwrap_or_else(|| panic!("missing Go oracle security case {name}"))
        .clone()
}

fn inline_manifest_fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata/inline-manifest")
}

#[test]
fn manifest_root_types_are_errors() {
    for (root, case) in [
        ("skill = \"wrong\"", "scalar_skill_root"),
        ("targets = \"wrong\"", "scalar_targets_root"),
        ("terms = \"wrong\"", "scalar_terms_root"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let root_path = temp.path().join("typed-skill");
        fs::create_dir_all(&root_path).expect("root");
        fs::write(
            root_path.join("SKILL.md"),
            "---\nname: typed-skill\ndescription: test\n---\nBody\n",
        )
        .expect("skill");
        fs::write(root_path.join("symskills.toml"), root).expect("manifest");
        let error = load_bundle(&root_path).expect_err("manifest root type");
        let expected = oracle_security_case(case);
        assert!(expected["rejected"].as_bool().unwrap());
        assert!(error.to_string().contains("expected a table"), "{error}");
    }
}

#[test]
fn manifest_inline_tables_match_go_oracle() {
    let bundle = load_bundle(&inline_manifest_fixture()).expect("inline manifest");
    let expected = oracle_bundle("inline-manifest");
    assert_eq!(
        serde_json::to_value(bundle.manifest).expect("serialize Rust manifest"),
        expected["manifest"],
    );
}

#[test]
fn missing_optional_overlay_trees_are_empty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("plain-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: plain-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    assert!(
        load_bundle(&root)
            .expect("no overlays")
            .block_overrides
            .is_empty()
    );

    fs::create_dir_all(root.join("overlays/claude")).expect("overlay target");
    assert!(
        load_bundle(&root)
            .expect("no overlay blocks")
            .block_overrides
            .is_empty()
    );
}

#[test]
fn relative_root_is_stored_as_an_absolute_logical_path() {
    let temp = tempfile::tempdir_in(".").expect("tempdir");
    let root = temp.path().join("relative-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: relative-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    let current = std::env::current_dir().expect("current directory");
    let relative = root.strip_prefix(current).expect("relative path");
    let bundle = load_bundle(relative).expect("relative root");
    assert!(bundle.root.is_absolute());
    assert_eq!(
        bundle.root.file_name().and_then(|name| name.to_str()),
        Some("relative-skill")
    );
}

#[cfg(unix)]
#[test]
fn internal_symlink_directories_are_enumerated_through_capability() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("safe-skill");
    fs::create_dir_all(root.join("real")).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(root.join("real/inner.txt"), "content\n").expect("resource");
    symlink("real", root.join("linked")).expect("directory symlink");
    let bundle = load_bundle(&root).expect("internal symlink directory");
    assert!(
        bundle
            .resources
            .iter()
            .any(|resource| resource.path == "linked/inner.txt")
    );
}

#[cfg(unix)]
#[test]
fn overlay_directory_symlink_is_not_enumerated_ambiently() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("safe-skill");
    let outside = temp.path().join("outside-overlays");
    fs::create_dir_all(&root).expect("root");
    fs::create_dir_all(outside.join("hermes/blocks")).expect("outside overlays");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(outside.join("hermes/blocks/worker.md"), "outside\n").expect("outside block");
    symlink(outside, root.join("overlays")).expect("overlay symlink");
    let error = load_bundle(&root).expect_err("overlay symlink must not be traversed");
    assert!(error.to_string().contains("overlays") || error.to_string().contains("escapes"));
}

#[cfg(unix)]
#[test]
fn in_root_overlay_block_symlink_is_checked_through_capability() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("safe-skill");
    let outside = temp.path().join("outside.md");
    fs::create_dir_all(root.join("overlays/hermes/blocks")).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(&outside, "outside\n").expect("outside");
    symlink(&outside, root.join("overlays/hermes/blocks/worker.md")).expect("block symlink");
    let error = load_bundle(&root).expect_err("overlay block escape");
    assert!(error.to_string().contains("escapes") || error.to_string().contains("path"));
}

#[cfg(unix)]
#[test]
fn binary_markdown_resources_preserve_bytes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("binary-skill");
    fs::create_dir_all(root.join("references")).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: binary-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(root.join("references/data.md"), [0xff, 0x00, b'x']).expect("binary markdown");
    let bundle = load_bundle(&root).expect("binary markdown resource");
    assert_eq!(bundle.markdown["references/data.md"], [0xff, 0x00, b'x']);
}

#[cfg(unix)]
#[test]
fn manifest_symlink_is_rejected_as_control_input() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("manifest-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: manifest-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    let outside = temp.path().join("outside.toml");
    fs::write(&outside, "[skill]\nversion = \"outside\"\n").expect("outside");
    symlink(&outside, root.join("symskills.toml")).expect("manifest symlink");
    let error = load_bundle(&root).expect_err("manifest symlink");
    assert!(error.to_string().contains("escapes skill root"));
}

#[cfg(unix)]
#[test]
fn control_file_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("control-skill");
    fs::create_dir_all(&root).expect("root");
    let outside = temp.path().join("outside.md");
    fs::write(
        &outside,
        "---\nname: control-skill\ndescription: test\n---\nBody\n",
    )
    .expect("outside");
    symlink(&outside, root.join("SKILL.md")).expect("skill symlink");
    let error = load_bundle(&root).expect_err("control symlink");
    assert!(
        error
            .to_string()
            .contains("escapes skill root or is not a regular file")
    );
}

#[test]
fn resource_size_boundary_is_enforced_before_materialization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("size-skill");
    fs::create_dir_all(&root).expect("root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: size-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    let boundary = vec![0_u8; usize::try_from(MAX_RESOURCE_SIZE).expect("test platform size")];
    fs::write(root.join("resource.bin"), &boundary).expect("boundary resource");
    load_bundle(&root).expect("exact boundary is accepted");
    let mut oversized = boundary;
    oversized.push(0);
    fs::write(root.join("resource.bin"), oversized).expect("oversized resource");
    let error = load_bundle(&root).expect_err("oversized resource");
    assert!(error.to_string().contains("maximum size"), "{error}");
}
