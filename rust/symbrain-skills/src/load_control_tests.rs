//! Prepared capability-only nofollow diagnostics and renamed-root controls.
use super::*;
use std::{fs, os::unix::fs::symlink};

fn cap(path: &Path) -> Dir {
    Dir::open_ambient_dir(path, ambient_authority()).unwrap()
}

#[test]
fn confined_final_link_retains_failed_nofollow_error_after_root_rename() {
    let owned = tempfile::tempdir().unwrap();
    let original = owned.path().join("original");
    fs::create_dir(&original).unwrap();
    fs::write(
        original.join("document.md"),
        b"not read by diagnostic probe",
    )
    .unwrap();
    symlink("document.md", original.join("SKILL.md")).unwrap();
    let root = cap(&original);
    fs::rename(&original, owned.path().join("renamed")).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("document.md"), b"replacement owner").unwrap();
    assert!(verified_control_link(&root, Path::new("SKILL.md")));
    let error = read_skill_document(&root, Path::new("SKILL.md"), "SKILL.md", None).unwrap_err();
    assert!(matches!(error, InputReadError::Read(_)));
    assert_eq!(
        error.to_string(),
        "read SKILL.md: openat SKILL.md: too many levels of symbolic links"
    );
    assert_eq!(
        fs::read(original.join("document.md")).unwrap(),
        b"replacement owner"
    );
}

#[test]
fn outside_dangling_loop_and_special_targets_remain_unverified_refusals() {
    let owned = tempfile::tempdir().unwrap();
    let original = owned.path().join("bundle");
    fs::create_dir(&original).unwrap();
    fs::write(owned.path().join("outside"), b"outside bytes never read").unwrap();
    symlink("../outside", original.join("outside-link")).unwrap();
    symlink("absent", original.join("dangling")).unwrap();
    symlink("loop", original.join("loop")).unwrap();
    #[cfg(target_os = "macos")]
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .args(["-m", "600"])
            .arg(original.join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    #[cfg(not(target_os = "macos"))]
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        original.join("fifo"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let socket = std::os::unix::net::UnixListener::bind(original.join("socket")).unwrap();
    let root = cap(&original);
    for target in ["outside-link", "dangling", "loop", "fifo", "socket"] {
        assert!(!verified_control_link(&root, Path::new(target)), "{target}");
        symlink(target, original.join("SKILL.md")).unwrap();
        let error =
            read_skill_document(&root, Path::new("SKILL.md"), "SKILL.md", None).unwrap_err();
        assert!(matches!(error, InputReadError::Rejected(_)));
        assert_eq!(
            error.to_string(),
            "SKILL.md escapes skill root or is not a regular file"
        );
        fs::remove_file(original.join("SKILL.md")).unwrap();
    }
    drop(socket);
    assert_eq!(
        fs::read(owned.path().join("outside")).unwrap(),
        b"outside bytes never read"
    );
}
