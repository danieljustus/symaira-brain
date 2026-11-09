//! Status scans of install roots below search-only ancestors (#924).
//!
//! The confined walk opens every ancestor read-only, which a search-only
//! (`--x`) directory refuses. The scan must read that as "no installed skills"
//! instead of aborting the whole report.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use symbrain_skills::install::{StatusOptions, status};

/// Restores permissions so the temporary directory can be removed.
struct Restore<'a>(&'a Path);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o755));
    }
}

fn scan(home: &Path) -> Result<usize, String> {
    status(&StatusOptions {
        home_dir: home.to_path_buf(),
        targets: vec!["claude".to_owned(), "opencode".to_owned()],
        ..Default::default()
    })
    .map(|rows| rows.len())
    .map_err(|error| error.to_string())
}

#[test]
fn search_only_ancestor_reports_no_installed_skills() {
    for mode in [0o311, 0o111] {
        for create_roots in [false, true] {
            let temp = tempfile::tempdir().expect("temp");
            let ancestor = temp.path().join("ancestor");
            let home = ancestor.join("home");
            fs::create_dir_all(&home).expect("home");
            if create_roots {
                fs::create_dir_all(home.join(".claude/skills")).expect("claude root");
                fs::create_dir_all(home.join(".config/opencode/skills")).expect("opencode root");
            }
            let _restore = Restore(&ancestor);
            fs::set_permissions(&ancestor, fs::Permissions::from_mode(mode)).expect("chmod");
            if fs::read_dir(&ancestor).is_ok() {
                // Privileged runners ignore directory permissions.
                return;
            }
            assert_eq!(
                scan(&home),
                Ok(0),
                "mode {mode:o}, existing roots {create_roots}"
            );
        }
    }
}

#[test]
fn readable_home_still_lists_installs() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join(".claude/skills/demo");
    fs::create_dir_all(&root).expect("unmanaged skill");
    assert_eq!(scan(temp.path()), Ok(1));
}
