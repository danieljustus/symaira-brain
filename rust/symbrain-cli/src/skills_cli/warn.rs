//! `source-dirty` warning (#1013): the library skill's own git repository has
//! uncommitted changes, so what gets rendered is not what is committed.
//!
//! Degrades silently: no `.git`, no `git` binary or a failing `git status`
//! all count as clean. Clean sources serialize with no extra field.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub(super) const SOURCE_DIRTY: &str = "source-dirty";

/// One report row plus the warnings found for its source directory.
#[derive(Debug, Serialize)]
pub(super) struct Warned<T: Serialize> {
    #[serde(flatten)]
    pub(super) row: T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<&'static str>,
    #[serde(skip)]
    dir: PathBuf,
}

/// Memoised dirty check, so one skill across many targets runs git once.
#[derive(Default)]
pub(super) struct Dirty(BTreeMap<PathBuf, bool>);

impl Dirty {
    pub(super) fn check(&mut self, dir: &Path) -> bool {
        *self.0.entry(dir.to_path_buf()).or_insert_with(|| {
            symbrain_skills::vcs::is_repo(dir) && symbrain_skills::vcs::dirty(dir).unwrap_or(false)
        })
    }

    pub(super) fn tag<T: Serialize>(&mut self, row: T, dir: PathBuf) -> Warned<T> {
        let warnings = if self.check(&dir) {
            vec![SOURCE_DIRTY]
        } else {
            Vec::new()
        };
        Warned { row, warnings, dir }
    }
}

/// Tags every row with the source directory `dir_of` names for it.
pub(super) fn tag_all<T: Serialize>(rows: &[T], dir_of: impl Fn(&T) -> PathBuf) -> Vec<Warned<&T>> {
    let mut dirty = Dirty::default();
    rows.iter().map(|row| dirty.tag(row, dir_of(row))).collect()
}

/// Prints one stderr line per dirty source directory (text-mode warning).
pub(super) fn print<T: Serialize>(verb: &str, rows: &[Warned<T>], stderr: &mut dyn Write) {
    let mut seen: Vec<&Path> = Vec::new();
    for row in rows.iter().filter(|row| !row.warnings.is_empty()) {
        if !seen.contains(&row.dir.as_path()) {
            seen.push(&row.dir);
            let _ = writeln!(
                stderr,
                "symbrain skills {verb}: warning: {SOURCE_DIRTY}: {} has uncommitted changes",
                row.dir.display()
            );
        }
    }
}
