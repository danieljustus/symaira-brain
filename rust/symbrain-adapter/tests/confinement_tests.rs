#![allow(missing_docs)]

#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
#[test]
fn adapter_rejects_a_symlinked_trusted_root() {
    let temp = tempfile::tempdir().expect("temporary root");
    let outside = tempfile::tempdir().expect("outside root");
    let link = temp.path().join("root-link");
    std::os::unix::fs::symlink(outside.path(), &link).expect("root symlink");
    assert!(
        symbrain_adapter::AtomicFile::open(&link, Path::new("AGENTS.md"), false).is_err(),
        "adapter followed a symlinked trusted root"
    );
    assert!(!outside.path().join("AGENTS.md").exists());
}
