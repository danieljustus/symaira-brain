//! Original sync directory/ownership assertions, moved without changes.

use super::*;
use crate::install::{InstallStatus, StatusKind};
use cap_std::ambient_authority;
use tempfile::tempdir;

#[test]
fn sync_dir_fsyncs_capability_directory_and_propagates_open_errors() {
    let temp = tempdir().expect("temporary directory");
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).expect("open root");
    root.create_dir("nested").expect("create nested directory");

    sync_dir(&root, Path::new("nested"), None).expect("directory fsync");

    std::fs::write(temp.path().join("not-a-directory"), b"x").expect("write fixture");
    let error = sync_dir(&root, Path::new("not-a-directory"), None)
        .expect_err("non-directory must not be treated as synced");
    assert!(error.0.contains("open directory for sync"));
}

#[test]
fn reinstall_locks_resolved_render_name() {
    let library = tempdir().expect("library");
    let home = tempdir().expect("home");
    let source = library.path().join("source");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: source\ndescription: test\n---\nbody\n",
    )
    .expect("skill");
    std::fs::write(
        source.join("symskills.toml"),
        "[targets.opencode]\nenabled = true\nalias = \"resolved\"\n",
    )
    .expect("manifest");
    let lock = super::super::sync_lock::pull_lock_path(home.path(), "opencode", "resolved")
        .expect("lock path");
    std::fs::create_dir_all(lock.parent().expect("lock parent")).expect("lock parent");
    std::fs::write(&lock, b"{\"pid\":1}\n").expect("held lock");

    let status = InstallStatus {
        target: "opencode".to_owned(),
        name: "source".to_owned(),
        path: home.path().join(".config/opencode/skills/source"),
        status: StatusKind::Stale,
        mode: Some("copy".to_owned()),
        installed_at: None,
        source_hash: None,
        allow_executable: None,
        error: None,
        drift: Vec::new(),
        render_status: None,
        render_drift: Vec::new(),
        render_error: None,
    };
    let options = SyncOptions {
        library_dir: library.path().to_path_buf(),
        home_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    let result = reinstall(
        &status,
        &options,
        "user",
        "copy".to_owned(),
        &BundleLoader::default(),
    );
    assert_eq!(result.action, "skipped", "{result:?}");
    assert!(
        result
            .error
            .contains("pull lock held for opencode/resolved")
    );
}
