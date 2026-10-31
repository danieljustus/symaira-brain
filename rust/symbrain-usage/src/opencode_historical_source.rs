//! Bind the frozen validator to its authentic immutable inputs, not today's schema.
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const REVISION: &str = "b807eae643c627ae31166b90176d5a0de6f4e647";

pub(super) struct HistoricalRoot(pub PathBuf);
impl Drop for HistoricalRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned historical validation inputs");
    }
}

pub(super) fn materialize(root: &Path, destination: PathBuf, paths: &[&str]) -> HistoricalRoot {
    let resolved = Command::new("git")
        .args(["-C", root.to_str().unwrap(), "rev-parse", REVISION])
        .output()
        .expect("historical Git object required");
    assert!(resolved.status.success());
    assert_eq!(String::from_utf8(resolved.stdout).unwrap().trim(), REVISION);
    std::fs::create_dir(&destination).expect("fresh owned validation directory");
    let owned = HistoricalRoot(destination);
    for path in paths {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["show", &format!("{REVISION}:{path}")])
            .output()
            .expect("read immutable historical input");
        assert!(output.status.success(), "historical input missing: {path}");
        let target = owned.0.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, output.stdout).unwrap();
    }
    owned
}
