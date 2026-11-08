//! Test-only helpers: a self-cleaning temp directory (the crate has no
//! `tempfile` dev-dependency) and file writers with controllable mtimes.

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A unique directory below `$TMPDIR`, removed on drop.
pub(crate) struct TestDir(pub PathBuf);

impl TestDir {
    pub(crate) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "symbrain-importer-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `body` to `relative` (creating parents) and returns its path.
    pub(crate) fn write(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write fixture");
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Sets the mtime of `path`.
pub(crate) fn touch(path: &Path, when: DateTime<Utc>) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open for mtime");
    let system: SystemTime = when.into();
    file.set_modified(system).expect("set mtime");
}
