//! A status scan must survive an install entry whose path has a non-directory
//! parent (#1022), while mutating callers stay fail-closed.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use symbrain_skills::install::{InstallOptions, StatusKind, StatusOptions, install_copy, status};

fn scan(home: &Path) -> Result<Vec<symbrain_skills::install::InstallStatus>, String> {
    status(&StatusOptions {
        home_dir: home.to_path_buf(),
        targets: vec!["claude".to_owned()],
        ..Default::default()
    })
    .map_err(|error| error.to_string())
}

#[test]
fn symlink_target_below_a_file_is_a_per_entry_problem() {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().join("home");
    let skills = home.join(".claude/skills");
    fs::create_dir_all(skills.join("plain")).expect("plain skill");
    let file = temp.path().join("file");
    fs::write(&file, b"x").expect("file");
    let file_link = temp.path().join("file-link");
    symlink(&file, &file_link).expect("link to file");
    // Parent of the target is a regular file / a symlink to a regular file.
    symlink(file.join("sub"), skills.join("below-file")).expect("link below file");
    symlink(file_link.join("sub"), skills.join("below-link")).expect("link below link");

    let rows = scan(&home).expect("scan completes");
    let names = rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["below-file", "below-link", "plain"]);
    for row in &rows {
        assert_eq!(row.status, StatusKind::Unmanaged, "{}", row.name);
    }
    let by_name = |name: &str| rows.iter().find(|row| row.name == name).expect(name);
    assert_eq!(by_name("plain").error, None);
    for (name, target) in [("below-file", &file), ("below-link", &file_link)] {
        let message = by_name(name).error.as_deref().expect("error names target");
        assert!(
            message.contains(&target.join("sub").display().to_string()),
            "{name}: {message}"
        );
        assert_eq!(by_name(name).path, skills.join(name));
    }
}

#[test]
fn missing_or_file_link_targets_keep_their_plain_unmanaged_rows() {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().join("home");
    let skills = home.join(".claude/skills");
    fs::create_dir_all(&skills).expect("skills");
    let file = temp.path().join("file");
    fs::write(&file, b"x").expect("file");
    symlink(&file, skills.join("to-file")).expect("link to file");
    symlink(temp.path().join("absent/deeper"), skills.join("dangling")).expect("dangling");

    let rows = scan(&home).expect("scan completes");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.status == StatusKind::Unmanaged && row.error.is_none())
    );
}

#[test]
fn install_with_a_file_parent_still_fails_closed() {
    let source = tempfile::tempdir().expect("source");
    fs::write(
        source.path().join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbody\n",
    )
    .expect("skill source");
    let home = tempfile::tempdir().expect("home");
    // `.config` is a file, so the opencode install root has a non-directory
    // ancestor.
    fs::write(home.path().join(".config"), b"x").expect("file ancestor");
    let result = install_copy(
        source.path(),
        "demo",
        "hash",
        &InstallOptions {
            home_dir: home.path().to_path_buf(),
            ..Default::default()
        },
    );
    assert!(result.is_err(), "install must not proceed below a file");
    assert!(home.path().join(".config").is_file());
}
