//! Read-only discovery of a first-run backup that was started but not finished.
//!
//! A cancelled or killed copy keeps its `.<app>-resume.partial` tree until a
//! later run finishes it and writes the `<app>.json` marker (see [`crate::ensure`]).
//! Diagnostics use [`pending`] to tell the user that such a backup is waiting.

use std::fs;
use std::path::{Path, PathBuf};

const PREFIX: &str = ".";
const SUFFIX: &str = "-resume.partial";

/// A first-run backup that has not been completed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Application whose state is being backed up (`symbrain`, `symguard`, ...).
    pub app: String,
    /// The resumable `.<app>-resume.partial` tree.
    pub partial: PathBuf,
    /// True while a live process holds the backup slot (the copy is running now).
    pub running: bool,
}

/// Lists unfinished backups in `store`, sorted by app. Never writes.
///
/// A partial tree whose `<app>.json` marker already exists is a leftover of a
/// finished backup, not a pending one, and is skipped.
#[must_use]
pub fn pending(store: &Path) -> Vec<Pending> {
    let Ok(entries) = fs::read_dir(store) else {
        return Vec::new();
    };
    let mut found: Vec<Pending> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let app = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?.to_owned();
            let is_tree = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let marked = fs::symlink_metadata(store.join(format!("{app}.json"))).is_ok();
            (!app.is_empty() && is_tree && !marked).then(|| Pending {
                running: crate::lock::is_held(store, &app),
                app,
                partial: entry.path(),
            })
        })
        .collect();
    found.sort_by(|left, right| left.app.cmp(&right.app));
    found
}

#[cfg(test)]
mod tests {
    use super::pending;
    use std::fs;

    #[test]
    fn reports_only_unmarked_partial_trees() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        fs::create_dir(root.join(".symbrain-resume.partial")).unwrap();
        fs::create_dir(root.join(".symguard-resume.partial")).unwrap();
        fs::write(root.join("symguard.json"), "{}").unwrap();
        fs::create_dir(root.join(".symbrain-20260101T000000Z.partial")).unwrap();
        fs::write(root.join(".other-resume.partial"), "file").unwrap();
        let found = pending(root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].app, "symbrain");
        assert!(!found[0].running);
        assert!(found[0].partial.ends_with(".symbrain-resume.partial"));
        assert!(
            !root.join(".symbrain.lock").exists(),
            "pending() must not create files"
        );
    }

    #[test]
    fn missing_store_is_empty() {
        assert!(pending(&std::path::PathBuf::from("/nonexistent/cutover-store")).is_empty());
    }

    #[test]
    fn live_slot_holder_marks_running() {
        let store = tempfile::tempdir().unwrap();
        fs::create_dir(store.path().join(".symbrain-resume.partial")).unwrap();
        let slot = crate::lock::try_acquire(store.path(), "symbrain")
            .unwrap()
            .unwrap();
        assert!(pending(store.path())[0].running);
        drop(slot);
        assert!(!pending(store.path())[0].running);
    }
}
