//! Native Windows pathname ownership and kernel-bound cleanup regression.
use super::*;
use std::time::Duration;

#[test]
fn render_lock_preserves_other_owners_and_releases_only_its_handle() {
    let temp = tempfile::tempdir().expect("owned root");
    let root = open_root(temp.path()).expect("root capability");
    let destination = Path::new("skill");
    let path = Path::new(".symskills-lock-skill");
    let acquire = || {
        lock_destination_windows(
            &root,
            Path::new("."),
            destination,
            Duration::from_millis(20),
        )
    };
    root.write(path, b"other owner's lock")
        .expect("pre-existing owner");
    assert_eq!(
        acquire().expect_err("existing owner must block").kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        root.read(path).expect("preserved owner"),
        b"other owner's lock"
    );
    root.remove_file(path).expect("fixture owner releases");

    let lock = acquire().expect("own lock");
    assert!(
        root.read_dir(".")
            .expect("entries")
            .any(|entry| entry.expect("entry").file_name() == path)
    );
    assert!(acquire().is_err(), "second owner must not acquire");
    assert!(
        root.remove_file(path).is_err(),
        "held handle must deny unlink/replacement"
    );
    drop(lock);
    assert!(
        !temp.path().join(path).exists(),
        "no residual pathname after clean release"
    );
    let next = acquire().expect("next owner after release");
    drop(next);
    assert!(!temp.path().join(path).exists());
}
