#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn sync_default_explicitly_skips_conflicts() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let source = library.path().join("demo");
    std::fs::create_dir_all(&source).expect("source directory");
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbase\n",
    )
    .expect("source");
    let bundle = symbrain_skills::load_bundle(&source).expect("bundle");
    let rendered = symbrain_skills::render_target(
        &bundle,
        "opencode",
        &symbrain_skills::RenderMetadata::default(),
    )
    .expect("render");
    symbrain_skills::install::install_rendered(
        &bundle,
        &rendered,
        &symbrain_skills::install::InstallOptions {
            home_dir: home.path().to_path_buf(),
            mode: "copy".to_owned(),
            ..Default::default()
        },
    )
    .expect("install");

    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nlibrary change\n",
    )
    .expect("library edit");
    std::fs::write(
        home.path().join(".config/opencode/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nharness change\n",
    )
    .expect("harness edit");

    let results = symbrain_skills::install::sync(&symbrain_skills::install::SyncOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action, "skipped");
    assert!(results[0].error.contains("conflict"));
    assert!(
        String::from_utf8(
            std::fs::read(home.path().join(".config/opencode/skills/demo/SKILL.md"))
                .expect("installed skill"),
        )
        .expect("utf8")
        .contains("harness change")
    );
}

fn write_skill(library: &std::path::Path, name: &str, body: &str, manifest: Option<&str>) {
    let root = library.join(name);
    std::fs::create_dir_all(&root).expect("skill directory");
    std::fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n{body}\n"),
    )
    .expect("skill source");
    if let Some(manifest) = manifest {
        std::fs::write(root.join("symskills.toml"), manifest).expect("skill manifest");
    }
}

fn install_fixture(
    library: &std::path::Path,
    home: &std::path::Path,
    name: &str,
    mode: &str,
    allow_executable: bool,
) {
    let bundle = symbrain_skills::load_bundle(&library.join(name)).expect("bundle");
    let rendered = symbrain_skills::render_target(
        &bundle,
        "opencode",
        &symbrain_skills::RenderMetadata::default(),
    )
    .expect("render");
    symbrain_skills::install::install_rendered(
        &bundle,
        &rendered,
        &symbrain_skills::install::InstallOptions {
            home_dir: home.to_path_buf(),
            mode: mode.to_owned(),
            allow_executable,
            ..Default::default()
        },
    )
    .expect("install");
}

fn install_project_fixture(
    library: &std::path::Path,
    home: &std::path::Path,
    project: &std::path::Path,
    name: &str,
) {
    let bundle = symbrain_skills::load_bundle(&library.join(name)).expect("bundle");
    let rendered = symbrain_skills::render_target(
        &bundle,
        "opencode",
        &symbrain_skills::RenderMetadata::default(),
    )
    .expect("render");
    symbrain_skills::install::install_rendered(
        &bundle,
        &rendered,
        &symbrain_skills::install::InstallOptions {
            home_dir: home.to_path_buf(),
            project_dir: Some(project.to_path_buf()),
            mode: "copy".to_owned(),
            ..Default::default()
        },
    )
    .expect("project install");
}

fn sync_options(
    library: &std::path::Path,
    home: &std::path::Path,
) -> symbrain_skills::install::SyncOptions {
    symbrain_skills::install::SyncOptions {
        home_dir: home.to_path_buf(),
        library_dir: library.to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    }
}

fn pull_lock_path(home: &std::path::Path, name: &str) -> std::path::PathBuf {
    home.join(".local/share/symskills/pending/.locks/opencode")
        .join(format!("{name}.lock"))
}

#[test]
fn sync_harness_changed_uses_go_pull_diagnostic() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    write_skill(library.path(), "edited", "body", None);
    install_fixture(library.path(), home.path(), "edited", "copy", false);
    std::fs::write(
        home.path().join(".config/opencode/skills/edited/SKILL.md"),
        "harness edit\n",
    )
    .expect("harness edit");

    let results =
        symbrain_skills::install::sync(&sync_options(library.path(), home.path())).expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].error, "harness changed; use symskills pull");
}

#[cfg(any(unix, windows))]
#[test]
fn sync_empty_mode_preserves_symlink_marker() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    write_skill(library.path(), "symlinked", "v1", None);
    install_fixture(library.path(), home.path(), "symlinked", "", false);
    std::fs::write(
        library.path().join("symlinked/SKILL.md"),
        "---\nname: symlinked\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");

    let results =
        symbrain_skills::install::sync(&sync_options(library.path(), home.path())).expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].mode.as_deref(), Some("symlink"));
    assert!(
        std::fs::symlink_metadata(&results[0].path)
            .expect("destination")
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn sync_manifest_allow_executable_is_or_with_marker_policy() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    write_skill(
        library.path(),
        "exec",
        "v1",
        Some("[skill]\nallow_executable = true\n"),
    );
    std::fs::write(library.path().join("exec/run.sh"), "#!/bin/sh\n").expect("script");
    std::fs::set_permissions(
        library.path().join("exec/run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("script mode");
    install_fixture(library.path(), home.path(), "exec", "copy", false);
    std::fs::write(
        library.path().join("exec/SKILL.md"),
        "---\nname: exec\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");

    let statuses = symbrain_skills::install::status(&symbrain_skills::install::StatusOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("status");
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].allow_executable, None);
    let results =
        symbrain_skills::install::sync(&sync_options(library.path(), home.path())).expect("sync");
    let mode = std::fs::metadata(results[0].path.join("run.sh"))
        .expect("installed script")
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "manifest executable policy was ignored");
}

#[test]
fn sync_empty_scope_with_project_dir_is_user_scope() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    write_skill(library.path(), "scoped", "v1", None);
    install_fixture(library.path(), home.path(), "scoped", "copy", false);
    std::fs::write(
        library.path().join("scoped/SKILL.md"),
        "---\nname: scoped\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");

    let mut options = sync_options(library.path(), home.path());
    options.project_dir = Some(project.path().to_path_buf());
    let results = symbrain_skills::install::sync(&options).expect("sync");
    assert_eq!(results.len(), 1);
    assert!(
        results[0]
            .path
            .starts_with(home.path().join(".config/opencode/skills"))
    );
    assert!(!project.path().join(".opencode/skills/scoped").exists());
}

#[test]
fn sync_project_scope_uses_absolute_project_root() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    write_skill(library.path(), "projected", "v1", None);
    install_project_fixture(library.path(), home.path(), project.path(), "projected");
    std::fs::write(
        library.path().join("projected/SKILL.md"),
        "---\nname: projected\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");

    let mut options = sync_options(library.path(), home.path());
    options.project_dir = Some(project.path().to_path_buf());
    options.scope = "project".to_owned();
    let results = symbrain_skills::install::sync(&options).expect("sync");
    assert_eq!(results.len(), 1);
    assert!(results[0].path.is_absolute());
    assert!(
        results[0]
            .path
            .starts_with(project.path().join(".opencode/skills"))
    );
}

#[test]
fn sync_skips_when_pull_lock_is_held() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    write_skill(library.path(), "locked", "v1", None);
    install_fixture(library.path(), home.path(), "locked", "copy", false);
    std::fs::write(
        library.path().join("locked/SKILL.md"),
        "---\nname: locked\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");
    let lock = pull_lock_path(home.path(), "locked");
    std::fs::create_dir_all(lock.parent().expect("lock parent")).expect("lock parent");
    std::fs::write(&lock, b"{\"pid\":1}\n").expect("held lock");

    let results =
        symbrain_skills::install::sync(&sync_options(library.path(), home.path())).expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action, "skipped");
    assert!(
        results[0]
            .error
            .contains("pull lock held for opencode/locked")
    );
}

#[test]
fn sync_recovers_stale_pull_lock() {
    use std::time::{Duration, SystemTime};

    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    write_skill(library.path(), "stale", "v1", None);
    install_fixture(library.path(), home.path(), "stale", "copy", false);
    std::fs::write(
        library.path().join("stale/SKILL.md"),
        "---\nname: stale\ndescription: test\n---\nv2\n",
    )
    .expect("library edit");
    let lock = pull_lock_path(home.path(), "stale");
    std::fs::create_dir_all(lock.parent().expect("lock parent")).expect("lock parent");
    let file = std::fs::File::create(&lock).expect("stale lock");
    file.set_modified(SystemTime::now() - Duration::from_secs(601))
        .expect("age stale lock");

    let results =
        symbrain_skills::install::sync(&sync_options(library.path(), home.path())).expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action, "installed");
    assert!(!lock.exists(), "sync must release recovered lock");
}

#[test]
fn sync_result_json_has_no_allow_executable_field() {
    let result = symbrain_skills::install::SyncResult {
        target: "opencode".to_owned(),
        name: "demo".to_owned(),
        path: "/tmp/demo".into(),
        action: "installed".to_owned(),
        mode: Some("copy".to_owned()),
        allow_executable: Some(true),
        error: String::new(),
    };
    let value = serde_json::to_value(result).expect("serialize result");
    assert!(value.get("allow_executable").is_none());
}
