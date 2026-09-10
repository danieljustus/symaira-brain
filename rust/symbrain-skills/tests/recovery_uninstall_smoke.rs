use std::fs;

use symbrain_skills::install::{InstallOptions, install_copy, install_rendered, uninstall};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

#[test]
fn project_uninstall_migrates_legacy_base_to_identity_tombstone() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    let source = library.path().join("demo");
    fs::create_dir_all(&source).expect("source");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbody\n",
    )
    .expect("skill");
    let bundle = load_bundle(&source).expect("bundle");
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
    let options = InstallOptions {
        home_dir: home.path().to_path_buf(),
        project_dir: Some(project.path().to_path_buf()),
        mode: "copy".to_owned(),
        ..Default::default()
    };
    install_rendered(&bundle, &rendered, &options).expect("install");

    let current = symbrain_skills::install::base_path_for_scope(
        home.path(),
        None,
        "opencode",
        "project",
        "demo",
        Some(project.path()),
    )
    .expect("current base");
    let legacy =
        symbrain_skills::install::legacy_project_base_path(home.path(), None, "opencode", "demo")
            .expect("legacy base");
    fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy root");
    fs::rename(&current, &legacy).expect("legacy snapshot");

    assert!(symbrain_skills::install::uninstall("opencode", "demo", &options).expect("uninstall"));
    assert!(!legacy.exists());
    assert!(current.with_extension("tombstone").is_file());
    assert!(!legacy.with_extension("tombstone").exists());
}

fn source() -> tempfile::TempDir {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("SKILL.md"),
        "---\nname: smoke\ndescription: x\n---\nbody\n",
    )
    .unwrap();
    source
}

#[test]
fn uninstall_is_transactional_and_writes_tombstone() {
    let source = source();
    let home = tempfile::tempdir().unwrap();
    let opts = InstallOptions {
        home_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    install_copy(source.path(), "smoke", "hash", &opts).unwrap();
    let destination = home.path().join(".config/opencode/skills/smoke");
    let base = home
        .path()
        .join(".local/share/symskills/base/opencode/smoke");
    assert!(uninstall("opencode", "smoke", &opts).unwrap());
    assert!(fs::symlink_metadata(&destination).is_err());
    assert!(fs::symlink_metadata(&base).is_err());
    assert!(
        home.path()
            .join(".local/share/symskills/base/opencode/smoke.tombstone")
            .is_file()
    );
}

#[test]
fn uninstall_dry_run_has_no_side_effects() {
    let source = source();
    let home = tempfile::tempdir().unwrap();
    let opts = InstallOptions {
        home_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    install_copy(source.path(), "smoke", "hash", &opts).unwrap();
    let dry = InstallOptions {
        home_dir: home.path().to_path_buf(),
        dry_run: true,
        ..Default::default()
    };
    assert!(uninstall("opencode", "smoke", &dry).unwrap());
    assert!(home.path().join(".config/opencode/skills/smoke").exists());
    assert!(
        !home
            .path()
            .join(".local/share/symskills/base/opencode/smoke.tombstone")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn symlink_install_reinstall_and_uninstall_keep_cache_transactional() {
    let source = source();
    let library = tempfile::tempdir().unwrap();
    let library_skill = library.path().join("smoke");
    fs::create_dir_all(&library_skill).unwrap();
    fs::copy(
        source.path().join("SKILL.md"),
        library_skill.join("SKILL.md"),
    )
    .unwrap();
    let bundle = load_bundle(&library_skill).unwrap();
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).unwrap();
    let home = tempfile::tempdir().unwrap();
    let opts = InstallOptions {
        home_dir: home.path().to_path_buf(),
        mode: "symlink".to_owned(),
        ..Default::default()
    };
    install_rendered(&bundle, &rendered, &opts).unwrap();
    let destination = home.path().join(".config/opencode/skills/smoke");
    assert!(
        fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    install_rendered(&bundle, &rendered, &opts).unwrap();
    assert!(uninstall("opencode", "smoke", &opts).unwrap());
    assert!(fs::symlink_metadata(&destination).is_err());
    assert!(home.path().join(".local/share/symskills/rendered").exists());
}
