//! Filesystem and path resolution tests for profiles.

use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

use symbrain_policy::constants::{
    MEMORY_MODE_READ_ONLY, SERVER_MEMORY, SERVER_SKILLS, SERVER_VAULT, VAULT_MODE_REQUEST_ONLY,
};
use symbrain_policy::profile::fs::{
    exists_in, list_names_in, load_all_in, load_file, load_from_dir,
};

fn setup_temp_profile(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().expect("tempdir failed");
    let profiles_dir = dir.path().join("profiles");
    fs::create_dir_all(&profiles_dir).expect("create_dir_all failed");
    fs::write(profiles_dir.join(format!("{name}.toml")), content).expect("write failed");
    (dir, profiles_dir)
}

#[test]
fn load_valid_file_from_dir() {
    let toml = r#"
[profile]
name = "personal"
description = "Full access"

[servers.vault]
enabled = true
mode = "request_only"
"#;
    let (_guard, profiles_dir) = setup_temp_profile("personal", toml);

    let p = load_from_dir(&profiles_dir, "personal").expect("load failed");
    assert_eq!(p.name, "personal");
    assert_eq!(p.description, "Full access");
    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_REQUEST_ONLY);
}

#[test]
fn load_missing_file_errors() {
    let dir = tempdir().unwrap();
    let profiles_dir = dir.path().join("profiles");

    let err = load_from_dir(&profiles_dir, "does-not-exist").unwrap_err();
    assert!(err.to_string().contains("failed to read"));
}

#[test]
fn load_path_traversal_name_rejected() {
    let dir = tempdir().unwrap();
    let err = load_from_dir(dir.path(), "../../etc/passwd").unwrap_err();
    assert!(err.to_string().contains("invalid name"));
}

#[test]
fn exists_reports_truthful_state() {
    let (_guard, profiles_dir) = setup_temp_profile("ghost", "[profile]\nname = \"ghost\"\n");
    assert!(exists_in(&profiles_dir, "ghost"));
    assert!(!exists_in(&profiles_dir, "non-existent"));
}

#[test]
fn list_names_sorted_and_ignores_non_toml() {
    let dir = tempdir().unwrap();
    let profiles_dir = dir.path().join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();

    // Empty initially
    let empty = list_names_in(&profiles_dir).unwrap();
    assert!(empty.is_empty());

    // Create files
    fs::write(profiles_dir.join("zeta.toml"), "[profile]\nname=\"zeta\"\n").unwrap();
    fs::write(
        profiles_dir.join("alpha.toml"),
        "[profile]\nname=\"alpha\"\n",
    )
    .unwrap();
    fs::write(profiles_dir.join("ignore.txt"), "not a toml").unwrap();
    fs::write(profiles_dir.join(".toml"), "").unwrap();
    fs::create_dir(profiles_dir.join("subdir.toml")).unwrap();

    let names = list_names_in(&profiles_dir).unwrap();
    assert_eq!(names, vec!["", "alpha", "zeta"]);
}

#[test]
fn load_all_reports_per_file_errors_without_failing_overall() {
    let dir = tempdir().unwrap();
    let profiles_dir = dir.path().join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();

    fs::write(profiles_dir.join("good.toml"), "[profile]\nname=\"good\"\n").unwrap();
    fs::write(
        profiles_dir.join("broken.toml"),
        "[profile]\nname=\"wrong-name\"\n",
    )
    .unwrap();

    let results = load_all_in(&profiles_dir).unwrap();
    assert_eq!(results.len(), 2);

    let good = results.iter().find(|r| r.name == "good").unwrap();
    assert!(good.profile.is_some());
    assert!(good.err.is_none());

    let broken = results.iter().find(|r| r.name == "broken").unwrap();
    assert!(broken.profile.is_none());
    assert!(broken.err.is_some());
}

#[test]
fn load_file_valid_room_local_profile() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("room-profile.toml");
    let content = r#"
[profile]
name        = "room"
description = "Room-local profile"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = false
"#;
    fs::write(&path, content).unwrap();

    let p = load_file(&path).unwrap();
    assert_eq!(p.name, "room");
    assert_eq!(p.description, "Room-local profile");
    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_REQUEST_ONLY);
    assert!(p.server(SERVER_MEMORY).enabled);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_ONLY);
    assert!(!p.server(SERVER_SKILLS).enabled);
}

#[test]
fn load_file_errors_matrix() {
    let dir = tempdir().unwrap();

    // 1. Missing file
    let missing_path = dir.path().join("nope.toml");
    assert!(load_file(&missing_path).is_err());

    // 2. Invalid TOML
    let broken_path = dir.path().join("broken.toml");
    fs::write(&broken_path, "[profile\nname =").unwrap();
    assert!(load_file(&broken_path).is_err());

    // 3. Missing name
    let noname_path = dir.path().join("noname.toml");
    fs::write(&noname_path, "[servers.vault]\nenabled = true\n").unwrap();
    let noname_err = load_file(&noname_path).unwrap_err();
    assert_eq!(
        noname_err.to_string(),
        format!(
            "profile \"\": invalid or missing name in {}: profile name \"\" must be non-empty and contain only letters, digits, '-', or '_'",
            noname_path.display()
        )
    );

    // 4. Unsafe name
    let unsafe_path = dir.path().join("unsafe.toml");
    fs::write(&unsafe_path, "[profile]\nname = \"../../evil\"\n").unwrap();
    let unsafe_err = load_file(&unsafe_path).unwrap_err();
    assert_eq!(
        unsafe_err.to_string(),
        format!(
            "profile \"../../evil\": invalid or missing name in {}: profile name \"../../evil\" must be non-empty and contain only letters, digits, '-', or '_'",
            unsafe_path.display()
        )
    );

    // 5. Name mismatch is impossible by construction with load_file
    let whatever_path = dir.path().join("whatever-name.toml");
    fs::write(&whatever_path, "[profile]\nname = \"room\"\n").unwrap();
    let p = load_file(&whatever_path).unwrap();
    assert_eq!(p.name, "room");
}
