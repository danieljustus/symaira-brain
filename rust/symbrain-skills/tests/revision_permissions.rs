//! Real archive extraction preserves creation umask and existing permissions.
#![cfg(unix)]
use std::io::{Cursor, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::Command;

#[test]
fn revision_file_modes_use_creation_permissions_without_later_chmod() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(repo.join("SKILL.md"), b"fixture bytes\n").unwrap();
    for args in [
        vec!["init"],
        vec!["add", "SKILL.md"],
        vec![
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    ] {
        let result = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let archive = Command::new("git")
        .args(["archive", "--format=tar", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(archive.status.success());
    let mut tar = tar::Archive::new(Cursor::new(archive.stdout));
    let archive_mode = tar
        .entries()
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.path().unwrap().as_ref() == std::path::Path::new("SKILL.md"))
        .unwrap()
        .header()
        .mode()
        .unwrap()
        & 0o777;
    let mut control = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(archive_mode)
        .open(temp.path().join("control"))
        .unwrap();
    control.write_all(b"mode control").unwrap();
    let expected = control.metadata().unwrap().permissions().mode() & 0o777;
    let fresh = temp.path().join("fresh");
    symbrain_skills::vcs::extract(&repo, "HEAD", &fresh).unwrap();
    assert_eq!(
        std::fs::metadata(fresh.join("SKILL.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        expected
    );
    std::fs::set_permissions(
        fresh.join("SKILL.md"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    symbrain_skills::vcs::extract(&repo, "HEAD", &fresh).unwrap();
    assert_eq!(
        std::fs::metadata(fresh.join("SKILL.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read(fresh.join("SKILL.md")).unwrap(),
        b"fixture bytes\n"
    );
}
