use std::fs;
use std::path::Path;

use symbrain_skills::install::{
    InstallOptions, StatusKind, StatusOptions, base_path_for_scope, install_path_for,
    install_rendered, read_manifest, status, uninstall,
};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

fn write_skill(root: &Path, body: &str) {
    fs::create_dir_all(root).expect("skill root");
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: demo\ndescription: test\n---\n{body}\n"),
    )
    .expect("skill source");
}

fn install_options(home: &Path, project: &Path) -> InstallOptions {
    InstallOptions {
        home_dir: home.to_path_buf(),
        project_dir: Some(project.to_path_buf()),
        mode: "copy".to_owned(),
        ..Default::default()
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn same_skill_name_has_independent_project_base_status_and_uninstall() {
    let home = tempfile::tempdir().expect("home");
    let project_one = tempfile::tempdir().expect("project one");
    let project_two = tempfile::tempdir().expect("project two");
    let library_one = tempfile::tempdir().expect("library one");
    let library_two = tempfile::tempdir().expect("library two");
    let source_one = library_one.path().join("demo");
    let source_two = library_two.path().join("demo");
    write_skill(&source_one, "project one");
    write_skill(&source_two, "project two");

    let bundle_one = load_bundle(&source_one).expect("bundle one");
    let bundle_two = load_bundle(&source_two).expect("bundle two");
    let rendered_one =
        render_target(&bundle_one, "opencode", &RenderMetadata::default()).expect("render one");
    let rendered_two =
        render_target(&bundle_two, "opencode", &RenderMetadata::default()).expect("render two");
    install_rendered(
        &bundle_one,
        &rendered_one,
        &install_options(home.path(), project_one.path()),
    )
    .expect("install one");
    install_rendered(
        &bundle_two,
        &rendered_two,
        &install_options(home.path(), project_two.path()),
    )
    .expect("install two");

    let base_one = base_path_for_scope(
        home.path(),
        None,
        "opencode",
        "project",
        "demo",
        Some(project_one.path()),
    )
    .expect("base one");
    let base_two = base_path_for_scope(
        home.path(),
        None,
        "opencode",
        "project",
        "demo",
        Some(project_two.path()),
    )
    .expect("base two");
    assert_ne!(base_one, base_two);
    assert_ne!(
        read_manifest(&base_one).expect("read base one"),
        read_manifest(&base_two).expect("read base two")
    );

    let status_options = |project: &Path, library: &Path| StatusOptions {
        home_dir: home.path().to_path_buf(),
        project_dir: Some(project.to_path_buf()),
        scope: "project".to_owned(),
        targets: vec!["opencode".to_owned()],
        library_dir: library.to_path_buf(),
        ..Default::default()
    };
    assert_eq!(
        status(&status_options(project_one.path(), library_one.path())).expect("status one")[0]
            .status,
        StatusKind::InSync
    );
    assert_eq!(
        status(&status_options(project_two.path(), library_two.path())).expect("status two")[0]
            .status,
        StatusKind::InSync
    );

    let destination_one = install_path_for(
        "opencode",
        home.path(),
        Some(project_one.path()),
        "project",
        "demo",
    )
    .expect("destination one");
    let destination_two = install_path_for(
        "opencode",
        home.path(),
        Some(project_two.path()),
        "project",
        "demo",
    )
    .expect("destination two");
    fs::write(
        destination_one.join("SKILL.md"),
        b"project one harness edit\n",
    )
    .expect("edit project one");
    assert_eq!(
        status(&status_options(project_one.path(), library_one.path())).expect("changed status")[0]
            .status,
        StatusKind::HarnessChanged
    );
    assert_eq!(
        status(&status_options(project_two.path(), library_two.path())).expect("isolated status")
            [0]
        .status,
        StatusKind::InSync
    );

    assert!(
        uninstall(
            "opencode",
            "demo",
            &install_options(home.path(), project_one.path())
        )
        .expect("uninstall one")
    );
    assert!(!destination_one.exists());
    assert!(!base_one.exists());
    assert!(destination_two.exists());
    assert!(base_two.exists());
    assert_eq!(
        status(&status_options(project_two.path(), library_two.path())).expect("remaining status")
            [0]
        .status,
        StatusKind::InSync
    );
    assert!(
        uninstall(
            "opencode",
            "demo",
            &install_options(home.path(), project_two.path())
        )
        .expect("uninstall two")
    );
}
