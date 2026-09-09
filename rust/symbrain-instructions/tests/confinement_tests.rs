#![allow(missing_docs)]
#![cfg_attr(unix, allow(unsafe_code))]

use std::path::Path;
#[cfg(unix)]
use symbrain_instructions::Source;

#[cfg(unix)]
#[test]
fn trusted_root_and_source_parent_symlinks_are_rejected() {
    let temp = tempfile::tempdir().expect("temporary root");
    let outside = tempfile::tempdir().expect("outside root");
    let root_link = temp.path().join("root-link");
    std::os::unix::fs::symlink(outside.path(), &root_link).expect("root symlink");
    assert!(
        symbrain_instructions::open_trusted_root(&root_link, false).is_err(),
        "trusted root symlink was followed"
    );

    let project_link = temp.path().join(".symbrain");
    std::os::unix::fs::symlink(outside.path(), &project_link).expect("source parent symlink");
    let source = Source::from_paths(
        temp.path().join("missing-global/instructions.md"),
        Some(project_link.join("instructions.md")),
    );
    assert!(
        source.content().is_err(),
        "source parent symlink was followed"
    );

    let home_real = temp.path().join("home-real");
    std::fs::create_dir_all(home_real.join(".config/symbrain")).expect("home tree");
    std::fs::write(
        home_real.join(".config/symbrain/instructions.md"),
        b"outside",
    )
    .expect("home source");
    let home_link = temp.path().join("home-link");
    std::os::unix::fs::symlink(&home_real, &home_link).expect("home symlink");
    let source = Source::with_environment(None, None, Some(&home_link));
    assert!(
        source.content().is_err(),
        "source followed a symlinked HOME"
    );
}

#[cfg(unix)]
#[test]
fn missing_source_parent_is_not_reopened_after_a_late_symlink() {
    let temp = tempfile::tempdir().expect("temporary root");
    let outside = tempfile::tempdir().expect("outside root");
    let missing = temp.path().join("late/instructions.md");
    let source = Source::from_paths(missing, None);
    std::fs::write(outside.path().join("instructions.md"), b"outside").expect("outside source");
    std::os::unix::fs::symlink(outside.path(), temp.path().join("late")).expect("late symlink");
    assert!(source.content().expect("missing source").is_empty());
}

#[cfg(unix)]
#[test]
fn fifo_source_is_rejected_without_blocking() {
    let temp = tempfile::tempdir().expect("temporary root");
    let path = temp.path().join("instructions.md");
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("fifo path");
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "mkfifo");
    let source = Source::from_paths(path, None);
    assert!(source.content().is_err(), "FIFO source was accepted");
}

#[test]
fn trusted_root_api_rejects_absolute_target_components() {
    let temp = tempfile::tempdir().expect("temporary root");
    assert!(symbrain_instructions::open_trusted_root(Path::new("/"), false).is_ok());
    assert!(symbrain_instructions::open_trusted_root(&temp.path().join("missing"), false).is_err());
}
