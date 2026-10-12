//! Verify the approved cache exception without omitting protected state.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(super) fn assert_only_cache_created(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
    case: &str,
) {
    let comparison = Path::new("cache/symbrain/skills/status-render");
    assert!(!before.keys().any(|path| path.starts_with(comparison)));
    assert!(after.keys().any(|path| {
        path.starts_with(comparison) && path.file_name() == Some(OsStr::new(".symskills.json"))
    }));
    for (path, bytes) in before {
        assert_eq!(
            after.get(path),
            Some(bytes),
            "{case}: changed {}",
            path.display()
        );
    }
    for path in after.keys().filter(|path| !before.contains_key(*path)) {
        assert!(
            path.starts_with(comparison)
                || ["cache", "cache/symbrain", "cache/symbrain/skills"]
                    .iter()
                    .any(|parent| path == Path::new(parent)),
            "{case}: wrote outside comparison cache: {}",
            path.display()
        );
    }
}
