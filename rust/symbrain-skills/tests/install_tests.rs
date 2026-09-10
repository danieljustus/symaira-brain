use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use symbrain_skills::install::{
    FaultPoint, InstallOptions, StatusKind, StatusOptions, classify_file, encode_marker,
    install_copy, install_rendered, status,
};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

fn write_source(root: &Path, body: &str) {
    fs::create_dir_all(root).expect("source root");
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: golden-fixture\ndescription: test\n---\n{body}"),
    )
    .expect("skill source");
    fs::write(root.join("reference.txt"), b"reference\n").expect("resource");
}

#[test]
fn install_copy_writes_marker_base_and_strips_executable_bits() {
    let source = tempfile::tempdir().expect("source");
    write_source(source.path(), "body\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            source.path().join("reference.txt"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("executable source");
    }
    let home = tempfile::tempdir().expect("home");
    let result = install_copy(
        source.path(),
        "demo",
        "source-hash",
        &InstallOptions {
            home_dir: home.path().to_path_buf(),
            ..Default::default()
        },
    )
    .expect("install");
    assert_eq!(result.action, "installed");
    assert_eq!(result.mode, "copy");
    assert_eq!(result.mode_changes.len(), 1);
    let destination = home.path().join(".config/opencode/skills/demo");
    assert_eq!(
        fs::read(destination.join("SKILL.md")).expect("installed skill"),
        fs::read(source.path().join("SKILL.md")).expect("source skill")
    );
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join(".symskills.json")).expect("marker"))
            .expect("marker JSON");
    assert_eq!(marker["mode"], "copy");
    assert_eq!(marker["source_hash"], "source-hash");
    assert!(
        home.path()
            .join(".local/share/symskills/base/opencode/demo/manifest.json")
            .is_file()
    );
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(destination.join("reference.txt"))
            .expect("installed resource")
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn reinstall_preserves_unknown_marker_fields_and_refuses_new_schema() {
    let source = tempfile::tempdir().expect("source");
    write_source(source.path(), "body\n");
    let home = tempfile::tempdir().expect("home");
    let options = InstallOptions {
        home_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    install_copy(source.path(), "demo", "hash", &options).expect("first install");
    let destination = home.path().join(".config/opencode/skills/demo");
    let mut marker: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&fs::read(destination.join(".symskills.json")).expect("marker"))
            .expect("object");
    marker.insert("future_field".to_owned(), serde_json::json!({"keep": true}));
    fs::write(
        destination.join(".symskills.json"),
        serde_json::to_vec(&marker).expect("marker bytes"),
    )
    .expect("marker update");
    install_copy(source.path(), "demo", "hash", &options).expect("reinstall");
    let preserved: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join(".symskills.json")).expect("marker"))
            .expect("marker JSON");
    assert_eq!(preserved["future_field"]["keep"], true);

    marker.insert("schema_version".to_owned(), serde_json::json!(99));
    fs::write(
        destination.join(".symskills.json"),
        serde_json::to_vec(&marker).expect("marker bytes"),
    )
    .expect("new marker");
    let error =
        install_copy(source.path(), "demo", "hash", &options).expect_err("new schema refusal");
    assert!(error.0.contains("newer than supported version"));
}

#[test]
fn dry_run_does_not_create_destination_or_base() {
    let source = tempfile::tempdir().expect("source");
    write_source(source.path(), "body\n");
    let home = tempfile::tempdir().expect("home");
    let result = install_copy(
        source.path(),
        "demo",
        "hash",
        &InstallOptions {
            home_dir: home.path().to_path_buf(),
            dry_run: true,
            ..Default::default()
        },
    )
    .expect("dry run");
    assert_eq!(result.action, "planned");
    assert!(!home.path().join(".config/opencode/skills/demo").exists());
    assert!(!home.path().join(".local/share/symskills/base").exists());
}

#[test]
fn replacement_faults_leave_old_destination_untouched() {
    let source = tempfile::tempdir().expect("source");
    write_source(source.path(), "new body\n");
    let home = tempfile::tempdir().expect("home");
    for fault in [FaultPoint::Write, FaultPoint::Sync, FaultPoint::Install] {
        let fault_home = tempfile::tempdir().expect("fault home");
        let options = InstallOptions {
            home_dir: fault_home.path().to_path_buf(),
            fault: Some(fault),
            ..Default::default()
        };
        assert!(install_copy(source.path(), "demo", "hash", &options).is_err());
        assert!(
            !fault_home
                .path()
                .join(".config/opencode/skills/demo")
                .exists()
        );
    }
    let clean = InstallOptions {
        home_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    write_source(source.path(), "old body\n");
    install_copy(source.path(), "demo", "old", &clean).expect("old install");
    write_source(source.path(), "new body\n");
    for fault in [
        FaultPoint::Backup,
        FaultPoint::Install,
        FaultPoint::RemoveBackup,
    ] {
        let options = InstallOptions {
            home_dir: home.path().to_path_buf(),
            fault: Some(fault),
            ..Default::default()
        };
        assert!(install_copy(source.path(), "demo", "new", &options).is_err());
        assert_eq!(
            fs::read(home.path().join(".config/opencode/skills/demo/SKILL.md"))
                .expect("old destination"),
            b"---\nname: golden-fixture\ndescription: test\n---\nold body\n"
        );
    }
}

#[test]
fn drift_table_and_status_are_deterministic() {
    use symbrain_skills::install::DriftKind;
    assert_eq!(classify_file("a", "a", "a"), DriftKind::Unchanged);
    assert_eq!(classify_file("a", "b", "a"), DriftKind::LibraryChanged);
    assert_eq!(classify_file("a", "a", "b"), DriftKind::HarnessChanged);
    assert_eq!(classify_file("a", "b", "b"), DriftKind::Converged);
    assert_eq!(classify_file("a", "b", "c"), DriftKind::Conflict);

    let library = tempfile::tempdir().expect("library");
    let skill = library.path().join("golden-fixture");
    write_source(&skill, "body\n");
    let bundle = load_bundle(&skill).expect("bundle");
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
    let home = tempfile::tempdir().expect("home");
    install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: home.path().to_path_buf(),
            ..Default::default()
        },
    )
    .expect("install rendered");
    let options = StatusOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        ..Default::default()
    };
    let rows = status(&options).expect("status");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, StatusKind::InSync);
    fs::write(
        home.path()
            .join(".config/opencode/skills/golden-fixture/SKILL.md"),
        b"harness edit\n",
    )
    .expect("harness edit");
    assert_eq!(
        status(&options).expect("harness status")[0].status,
        StatusKind::HarnessChanged
    );
    fs::write(
        skill.join("SKILL.md"),
        b"---\nname: golden-fixture\ndescription: changed\n---\nsource edit\n",
    )
    .expect("library edit");
    assert_eq!(
        status(&options).expect("conflict status")[0].status,
        StatusKind::Conflict
    );
}

#[test]
fn marker_round_trip_keeps_unknown_fields() {
    let marker = serde_json::json!({
        "schema_version": 1,
        "managed_by": "symskills",
        "target": "opencode",
        "name": "demo",
        "rendered_at": "source",
        "mode": "copy",
        "installed": "now",
        "future": [1, 2, 3]
    });
    let state = symbrain_skills::install::parse_marker(&serde_json::to_vec(&marker).expect("JSON"))
        .expect("parse");
    let symbrain_skills::install::MarkerState::Valid(parsed) = state else {
        panic!("valid marker")
    };
    let encoded = encode_marker(&parsed).expect("encode");
    let round_trip: serde_json::Value = serde_json::from_slice(&encoded).expect("encoded JSON");
    assert_eq!(round_trip["future"], serde_json::json!([1, 2, 3]));
}
