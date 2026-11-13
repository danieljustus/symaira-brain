//! #859: a copy install that names a render root retains the rendered tree.
use std::fs;
use std::path::Path;

use symbrain_skills::install::{InstallOptions, install_rendered};
use symbrain_skills::{Bundle, RenderMetadata, Rendered, load_bundle, render_target};

fn fixture(root: &Path) -> (Bundle, Rendered) {
    let source = root.join("library/example");
    fs::create_dir_all(source.join("references")).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: example\ndescription: copy retention\n---\nBody\n",
    )
    .unwrap();
    fs::write(source.join("references/a.md"), "A\n").unwrap();
    let bundle = load_bundle(&source).unwrap();
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).unwrap();
    (bundle, rendered)
}

fn rendered_at(installed: &Path) -> String {
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(installed.join(".symskills.json")).unwrap()).unwrap();
    marker["rendered_at"].as_str().unwrap().to_owned()
}

#[test]
fn copy_with_render_root_retains_the_tree_the_marker_names() {
    let root = tempfile::tempdir().unwrap();
    let (bundle, rendered) = fixture(root.path());
    let render_dir = root.path().join("rendered");
    let result = install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: root.path().join("home"),
            render_dir: Some(render_dir.clone()),
            mode: "copy".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let retained = render_dir.join("opencode/example");
    assert!(!fs::symlink_metadata(&result.path).unwrap().is_symlink());
    assert_eq!(
        fs::read(retained.join("SKILL.md")).unwrap(),
        fs::read(result.path.join("SKILL.md")).unwrap()
    );
    assert_eq!(fs::read(retained.join("references/a.md")).unwrap(), b"A\n");
    assert_eq!(Path::new(&rendered_at(&result.path)), retained);
    assert!(retained.exists(), "marker must not name a deleted path");
}

#[test]
fn copy_without_render_root_and_dry_run_retain_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (bundle, rendered) = fixture(root.path());
    let render_dir = root.path().join("rendered");
    install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: root.path().join("home-dry"),
            render_dir: Some(render_dir.clone()),
            mode: "copy".into(),
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!render_dir.exists(), "dry run must not publish a render");

    let result = install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: root.path().join("home"),
            mode: "copy".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.path.join("SKILL.md").is_file());
    assert!(!render_dir.exists());
}
