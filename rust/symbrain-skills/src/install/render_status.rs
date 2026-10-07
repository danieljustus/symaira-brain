//! Read-only library-versus-render reports; never promote cache edits to SSOT.
use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use super::destination::entry_metadata;
use super::drift::file_hashes;
use super::{InstallStatus, StatusOptions};
use crate::SkillError;

/// State of a retained render compared with a fresh render of its library skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderStatus {
    /// All content paths and bytes agree.
    InSync,
    /// At least one path was added, removed, or changed.
    Drift,
    /// Confinement, type, size, or I/O checks prevented comparison.
    Unreadable,
}

impl RenderStatus {
    /// Stable table label, matching the JSON vocabulary.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::InSync => "in-sync",
            Self::Drift => "drift",
            Self::Unreadable => "unreadable",
        }
    }
}

/// A changed path; empty hashes mean that side has no file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderDrift {
    /// Slash-separated relative path in deterministic order.
    pub path: String,
    /// Hash of the freshly rendered library version, including target transforms.
    pub library_hash: String,
    /// Hash of the retained rendered file.
    pub render_hash: String,
}

pub(super) fn inspect(
    row: &mut InstallStatus,
    options: &StatusOptions,
    installed: &Path,
    fresh: &Path,
) {
    let Some(root) = &options.render_dir else {
        return;
    };
    let cached = root.join(&row.target).join(&row.name);
    let result = (|| -> Result<(), SkillError> {
        if entry_metadata(&cached)?.is_none() {
            return Ok(());
        }
        // Existing bounded hashing rejects links/special files at every level;
        // no canonicalization or writes expand the cache's authority.
        let library = file_hashes(fresh, false)?;
        let render = file_hashes(&cached, false)?;
        let paths: BTreeSet<_> = library.keys().chain(render.keys()).collect();
        row.render_drift = paths
            .into_iter()
            .filter(|path| library.get(*path) != render.get(*path))
            .map(|path| RenderDrift {
                path: path.clone(),
                library_hash: library.get(path).cloned().unwrap_or_default(),
                render_hash: render.get(path).cloned().unwrap_or_default(),
            })
            .collect();
        row.render_status = Some(if row.render_drift.is_empty() {
            RenderStatus::InSync
        } else {
            RenderStatus::Drift
        });
        // This is a presentation mode only. Persisted markers and sync options
        // remain `symlink`; other destinations retain their ordinary mode.
        if row.mode.as_deref() == Some("symlink")
            && let (Ok(installed), Ok(cached)) =
                (std::path::absolute(installed), std::path::absolute(&cached))
            && installed == cached
        {
            row.mode = Some("linked".to_owned());
        }
        Ok(())
    })();
    if let Err(error) = result {
        row.render_status = Some(RenderStatus::Unreadable);
        row.render_error = Some(error.0);
    }
}
