use std::fs;

use symbrain_skills::install::{
    InstallOptions, StatusKind, StatusOptions, install_rendered, status,
};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

fn write_skill(root: &std::path::Path, name: &str, body: &str) {
    fs::create_dir_all(root).expect("skill root");
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n{body}"),
    )
    .expect("skill source");
}

fn install_one(home: &std::path::Path, source: &std::path::Path, target: &str) {
    let bundle = load_bundle(source).expect("bundle");
    let rendered = render_target(&bundle, target, &RenderMetadata::default()).expect("render");
    install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: home.to_path_buf(),
            mode: "copy".to_owned(),
            ..Default::default()
        },
    )
    .expect("install");
}

#[test]
fn status_sorts_rows_by_target_then_name_like_go() {
    let home = tempfile::tempdir().expect("home");
    let library = tempfile::tempdir().expect("library");
    let source = library.path().join("sorted");
    write_skill(&source, "sorted", "body\n");
    install_one(home.path(), &source, "opencode");
    install_one(home.path(), &source, "claude");

    let rows = status(&StatusOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned(), "claude".to_owned()],
        ..Default::default()
    })
    .expect("status");
    assert_eq!(
        rows.iter()
            .map(|row| row.target.as_str())
            .collect::<Vec<_>>(),
        ["claude", "opencode"]
    );
}

#[test]
fn status_keeps_broken_managed_install_as_stale_row() {
    let home = tempfile::tempdir().expect("home");
    let library = tempfile::tempdir().expect("library");
    let source = library.path().join("broken");
    write_skill(&source, "broken", "body\n");
    install_one(home.path(), &source, "opencode");
    fs::write(source.join("SKILL.md"), "not valid frontmatter").expect("break source");

    let rows = status(&StatusOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("status should retain row");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, StatusKind::Stale);
    assert!(rows[0].error.is_some());
}

#[test]
fn status_unmanaged_identity_preserves_marker_metadata() {
    let home = tempfile::tempdir().expect("home");
    let library = tempfile::tempdir().expect("library");
    let source = library.path().join("identity");
    write_skill(&source, "identity", "body\n");
    install_one(home.path(), &source, "opencode");
    let marker_path = home
        .path()
        .join(".config/opencode/skills/identity/.symskills.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker_path).expect("marker")).expect("marker JSON");
    marker["target"] = serde_json::Value::String("other".to_owned());
    fs::write(
        &marker_path,
        serde_json::to_vec(&marker).expect("marker bytes"),
    )
    .expect("rewrite marker");

    let rows = status(&StatusOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("status");
    assert_eq!(rows[0].status, StatusKind::Unmanaged);
    assert_eq!(rows[0].mode.as_deref(), Some("copy"));
    assert!(rows[0].installed_at.is_some());
    assert!(rows[0].source_hash.is_some());
}
