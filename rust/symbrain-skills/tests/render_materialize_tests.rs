use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{Arc, Barrier};
use symbrain_skills::{
    MAX_INPUT_SIZE, RenderMetadata, default_targets, load_bundle, materialize,
    materialize_with_fault, render_target,
};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata")
        .join(name)
}

#[cfg(unix)]
#[test]
fn materialization_does_not_follow_marker_symlink() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let first = materialize(&bundle, &rendered, output.path()).expect("first materialize");
    let outside = tempfile::tempdir().expect("outside");
    let outside_marker = outside.path().join("marker.json");
    let original = fs::read(first.root.join(".symskills.json")).expect("marker");
    fs::write(&outside_marker, &original).expect("outside marker");
    fs::remove_file(first.root.join(".symskills.json")).expect("remove marker");
    symlink(&outside_marker, first.root.join(".symskills.json")).expect("marker symlink");

    let second = materialize(&bundle, &rendered, output.path()).expect("second materialize");
    let metadata =
        fs::symlink_metadata(second.root.join(".symskills.json")).expect("marker metadata");
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(fs::read(&outside_marker).expect("outside marker"), original);
}

#[cfg(unix)]
#[test]
fn materialization_rejects_destination_symlink_escapes() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    for final_path in [false, true] {
        let output = tempfile::tempdir().expect("output");
        let outside = tempfile::tempdir().expect("outside");
        let target_parent = output.path().join("opencode");
        let link = if final_path {
            fs::create_dir(&target_parent).expect("target parent");
            target_parent.join(&rendered.name)
        } else {
            target_parent.clone()
        };
        symlink(outside.path(), link).expect("destination symlink");
        assert!(materialize(&bundle, &rendered, output.path()).is_err());
        assert!(
            outside
                .path()
                .read_dir()
                .expect("outside entries")
                .next()
                .is_none()
        );
    }
}

#[test]
fn materialization_faults_preserve_old_tree() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let first = materialize(&bundle, &rendered, output.path()).expect("first materialize");
    fs::write(first.root.join("SKILL.md"), b"old skill\n").expect("old skill");
    let mut changed = rendered.clone();
    changed.skill_md = b"changed skill\n".to_vec();

    for operation in ["write", "sync-dir", "swap-backup", "swap-install"] {
        let error = materialize_with_fault(&bundle, &changed, output.path(), Some(operation))
            .expect_err("injected materialization fault");
        assert!(
            error.0.contains("injected materialization fault"),
            "{error}"
        );
        assert_eq!(
            fs::read(first.root.join("SKILL.md")).expect("preserved skill"),
            b"old skill\n",
            "old tree changed for {operation}"
        );
    }

    let error =
        materialize_with_fault(&bundle, &changed, output.path(), Some("swap-remove-backup"))
            .expect_err("backup cleanup failure must be reported");
    assert!(error.0.contains("swap-remove-backup"), "{error}");
    assert_eq!(
        fs::read(first.root.join("SKILL.md")).expect("restored skill"),
        b"old skill\n"
    );
}

#[test]
fn materialization_rebuilds_poisoned_output_manifest() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("OpenCode render");
    let output = tempfile::tempdir().expect("output");
    let first = materialize(&bundle, &rendered, output.path()).expect("first materialize");
    fs::write(first.root.join("scripts/helper.sh"), b"poisoned\n").expect("poison support");
    fs::write(first.root.join("extra.txt"), b"unexpected\n").expect("extra output");

    let rebuilt = materialize(&bundle, &rendered, output.path()).expect("rebuild");
    assert_eq!(
        fs::read(rebuilt.root.join("scripts/helper.sh")).expect("support"),
        fs::read(fixture("golden/opencode/scripts/helper.sh")).expect("golden support")
    );
    assert!(!rebuilt.root.join("extra.txt").exists());
}

#[test]
fn materialization_keeps_nested_marker_as_resource() {
    let root = tempfile::tempdir().expect("bundle root");
    fs::write(
        root.path().join("SKILL.md"),
        "---\nname: nested-marker\ndescription: test\n---\nBody.\n",
    )
    .expect("SKILL.md");
    let nested = br#"{"source_hash":"poison","output_manifest":"ignore"}"#;
    fs::create_dir(root.path().join("references")).expect("references");
    fs::write(root.path().join("references/.symskills.json"), nested).expect("nested marker");
    let bundle = load_bundle(root.path()).expect("bundle");
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
    let output = tempfile::tempdir().expect("output");
    let materialized = materialize(&bundle, &rendered, output.path()).expect("materialize");
    assert_eq!(
        fs::read(materialized.root.join("references/.symskills.json")).expect("nested output"),
        nested
    );
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(materialized.root.join(".symskills.json")).expect("root marker"),
    )
    .expect("root marker JSON");
    assert_ne!(marker["source_hash"], "poison");
}

#[test]
fn materialization_rejects_oversized_preserved_metadata() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
    let output = tempfile::tempdir().expect("output");
    let first = materialize(&bundle, &rendered, output.path()).expect("first materialize");
    fs::write(first.root.join("SKILL.md"), b"old skill\n").expect("old skill");
    let metadata = "x".repeat(usize::try_from(MAX_INPUT_SIZE).expect("test platform") - 512);
    fs::write(
        first.root.join(".symskills.json"),
        serde_json::to_vec(&serde_json::json!({"metadata": metadata})).expect("metadata"),
    )
    .expect("oversized metadata marker");
    let mut changed = rendered.clone();
    changed.skill_md = b"changed skill\n".to_vec();
    let error = materialize(&bundle, &changed, output.path()).expect_err("metadata bound");
    assert!(
        error.0.contains("render marker exceeds maximum input size"),
        "{error}"
    );
    assert_eq!(
        fs::read(first.root.join("SKILL.md")).expect("preserved skill"),
        b"old skill\n"
    );
}

#[test]
fn materialization_serializes_concurrent_same_destination() {
    let bundle = Arc::new(load_bundle(&fixture("source")).expect("source fixture"));
    let rendered =
        Arc::new(render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render"));
    let output = tempfile::tempdir().expect("output");
    let output_path = output.path().to_path_buf();
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let bundle = Arc::clone(&bundle);
        let rendered = Arc::clone(&rendered);
        let barrier = Arc::clone(&barrier);
        let output = output_path.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            materialize(&bundle, &rendered, &output).expect("serialized materialize");
        }));
    }
    for worker in workers {
        worker.join().expect("worker");
    }
    assert!(
        output_path
            .join("opencode")
            .join(&rendered.name)
            .join("SKILL.md")
            .is_file()
    );
}

#[test]
fn all_source_targets_match_go_golden_trees() {
    let bundle = load_bundle(&fixture("source")).expect("source fixture");
    for target in default_targets() {
        let rendered = render_target(&bundle, &target, &RenderMetadata::default())
            .unwrap_or_else(|error| panic!("{target}: render: {error}"));
        let output = tempfile::tempdir().expect("output");
        let materialized = materialize(&bundle, &rendered, output.path())
            .unwrap_or_else(|error| panic!("{target}: materialize: {error}"));
        assert_tree_matches(
            &materialized.root,
            &fixture(&format!("golden/{target}")),
            &target,
        );
    }
}

#[test]
fn all_variant_targets_match_go_golden_trees() {
    let bundle = load_bundle(&fixture("variant-source")).expect("variant fixture");
    for target in default_targets() {
        let rendered = render_target(&bundle, &target, &RenderMetadata::default())
            .unwrap_or_else(|error| panic!("{target}: render: {error}"));
        let output = tempfile::tempdir().expect("output");
        let materialized = materialize(&bundle, &rendered, output.path())
            .unwrap_or_else(|error| panic!("{target}: materialize: {error}"));
        assert_tree_matches(
            &materialized.root,
            &fixture(&format!("variant-golden/{target}")),
            &target,
        );
    }
}

fn assert_tree_matches(actual: &std::path::Path, expected: &std::path::Path, target: &str) {
    let collect = |root: &std::path::Path| {
        let mut files = std::collections::BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current).expect("read golden tree") {
                let entry = entry.expect("golden entry");
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .expect("relative golden path")
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/");
                    files.insert(relative, fs::read(path).expect("read golden file"));
                }
            }
        }
        files
    };
    let actual_files = collect(actual);
    let expected_files = collect(expected);
    assert_eq!(
        actual_files.keys().collect::<Vec<_>>(),
        expected_files.keys().collect::<Vec<_>>(),
        "{target}: golden file set drift"
    );
    for (path, expected_bytes) in expected_files {
        let actual_bytes = &actual_files[&path];
        assert_eq!(
            actual_bytes,
            &expected_bytes,
            "{target}: {path} drift (actual {} bytes, expected {} bytes)",
            actual_bytes.len(),
            expected_bytes.len()
        );
    }
}
