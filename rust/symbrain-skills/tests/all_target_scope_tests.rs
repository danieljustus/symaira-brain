use std::{fs, path::Path};

use symbrain_skills::install::{
    InstallOptions, StatusKind, StatusOptions, SyncOptions, install_rendered, status, sync,
    uninstall,
};
use symbrain_skills::{RenderMetadata, default_targets, load_bundle, render_target};

#[test]
fn all_registered_targets_support_project_scope_install_and_status() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");

    for target in default_targets() {
        let name = format!("skill-{target}");
        let source = library.path().join(&name);
        fs::create_dir_all(&source).expect("source directory");
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\nbody\n"),
        )
        .expect("skill source");
        let bundle = load_bundle(&source).expect("bundle");
        let rendered = render_target(&bundle, &target, &RenderMetadata::default())
            .unwrap_or_else(|error| panic!("render {target}: {error}"));
        install_rendered(
            &bundle,
            &rendered,
            &InstallOptions {
                home_dir: home.path().to_path_buf(),
                project_dir: Some(project.path().to_path_buf()),
                mode: "copy".to_owned(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("install {target}: {error}"));
    }

    let rows = status(&StatusOptions {
        home_dir: home.path().to_path_buf(),
        project_dir: Some(project.path().to_path_buf()),
        scope: "project".to_owned(),
        library_dir: library.path().to_path_buf(),
        ..Default::default()
    })
    .expect("status");
    assert!(
        default_targets().iter().all(|target| {
            rows.iter().any(|row| {
                row.target == *target
                    && row.name == format!("skill-{target}")
                    && row.status == StatusKind::InSync
            })
        }),
        "each target's own project install must remain in sync",
    );
}

#[test]
fn lifecycle_matrix_covers_all_targets_scopes_modes() {
    for target in default_targets() {
        for (scope, project_dir) in [
            ("user", None),
            ("project", Some(tempfile::tempdir().expect("project"))),
        ] {
            for mode in ["copy", "symlink"] {
                run_lifecycle_case(
                    &target,
                    scope,
                    project_dir.as_ref().map(tempfile::TempDir::path),
                    mode,
                );
            }
        }
    }
}

fn run_lifecycle_case(target: &str, scope: &str, project_dir: Option<&Path>, mode: &str) {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let project_path = project_dir.map(Path::to_path_buf);
    let name = format!("matrix-{target}-{scope}-{mode}");
    let source = library.path().join(&name);
    fs::create_dir_all(&source).expect("source directory");
    write_matrix_skill(&source, &name, "body-v1");
    let bundle = load_bundle(&source).expect("bundle");
    let rendered = render_target(&bundle, target, &RenderMetadata::default())
        .unwrap_or_else(|error| panic!("render {target}/{scope}/{mode}: {error}"));
    let options = InstallOptions {
        home_dir: home.path().to_path_buf(),
        project_dir: project_path.clone(),
        mode: mode.to_owned(),
        ..Default::default()
    };
    install_rendered(&bundle, &rendered, &options)
        .unwrap_or_else(|error| panic!("install {target}/{scope}/{mode}: {error}"));
    let status_options = || StatusOptions {
        home_dir: home.path().to_path_buf(),
        project_dir: project_path.clone(),
        scope: scope.to_owned(),
        targets: vec![target.to_owned()],
        library_dir: library.path().to_path_buf(),
        ..Default::default()
    };
    let rows = status(&status_options()).expect("initial status");
    assert_eq!(rows.len(), 1, "one matrix status row");
    assert_eq!(rows[0].status, StatusKind::InSync);

    write_matrix_skill(&source, &name, "body-v2");
    let dry_sync = sync(&SyncOptions {
        library_dir: library.path().to_path_buf(),
        home_dir: home.path().to_path_buf(),
        project_dir: project_path.clone(),
        scope: scope.to_owned(),
        targets: vec![target.to_owned()],
        dry_run: true,
        ..Default::default()
    })
    .expect("dry-run sync");
    assert_eq!(dry_sync.len(), 1);
    assert_eq!(dry_sync[0].action, "planned");
    assert_eq!(
        status(&status_options()).expect("stale status")[0].status,
        StatusKind::Stale
    );

    let synced = sync(&SyncOptions {
        library_dir: library.path().to_path_buf(),
        home_dir: home.path().to_path_buf(),
        project_dir: project_path.clone(),
        scope: scope.to_owned(),
        targets: vec![target.to_owned()],
        ..Default::default()
    })
    .expect("sync");
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].action, "installed");
    assert_eq!(
        status(&status_options()).expect("synced status")[0].status,
        StatusKind::InSync
    );

    force_install_and_uninstall(
        target,
        &name,
        &source,
        home.path(),
        project_path.as_deref(),
        scope,
        mode,
    );
}

fn force_install_and_uninstall(
    target: &str,
    name: &str,
    source: &Path,
    home: &Path,
    project_dir: Option<&Path>,
    scope: &str,
    mode: &str,
) {
    let destination =
        symbrain_skills::install::install_path_for(target, home, project_dir, scope, name)
            .expect("destination");
    if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
        if fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.is_dir()) {
            fs::remove_dir_all(&destination).expect("remove managed destination");
        } else {
            fs::remove_file(&destination).expect("remove managed destination");
        }
    }
    fs::create_dir_all(&destination).expect("unmanaged destination");
    fs::write(destination.join("unmanaged.txt"), b"unmanaged").expect("unmanaged file");
    write_matrix_skill(source, name, "body-v3");
    let fresh_bundle = load_bundle(source).expect("fresh bundle");
    let fresh_rendered =
        render_target(&fresh_bundle, target, &RenderMetadata::default()).expect("fresh render");
    let forced = InstallOptions {
        home_dir: home.to_path_buf(),
        project_dir: project_dir.map(Path::to_path_buf),
        mode: mode.to_owned(),
        force: true,
        ..Default::default()
    };
    let forced_result =
        install_rendered(&fresh_bundle, &fresh_rendered, &forced).expect("force adoption");
    assert!(
        forced_result.backup_path.is_some(),
        "force must retain backup"
    );

    let dry_uninstall = InstallOptions {
        home_dir: home.to_path_buf(),
        project_dir: project_dir.map(Path::to_path_buf),
        mode: mode.to_owned(),
        dry_run: true,
        ..Default::default()
    };
    assert!(uninstall(target, name, &dry_uninstall).expect("dry uninstall"));
    assert!(fs::symlink_metadata(&destination).is_ok());
    assert!(uninstall(target, name, &forced).expect("uninstall"));
    assert!(fs::symlink_metadata(&destination).is_err());
}

fn write_matrix_skill(source: &Path, name: &str, body: &str) {
    fs::write(
        source.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: matrix test\n---\n{body}\n"),
    )
    .expect("matrix skill");
}
