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
        if row.mode.as_deref() == Some("symlink") && same_cached_install(installed, &cached) {
            row.mode = Some("linked".to_owned());
        }
        Ok(())
    })();
    if let Err(error) = result {
        row.render_status = Some(RenderStatus::Unreadable);
        row.render_error = Some(error.0);
    }
}

#[cfg(windows)]
fn same_cached_install(installed: &Path, cached: &Path) -> bool {
    // Windows canonical paths use the extended-length prefix; canonicalize
    // both spellings before comparing the resolved install and render cache.
    matches!(
        (std::fs::canonicalize(installed), std::fs::canonicalize(cached)),
        (Ok(installed), Ok(cached)) if installed == cached
    )
}

#[cfg(not(windows))]
fn same_cached_install(installed: &Path, cached: &Path) -> bool {
    // Presentation only, after bounded no-follow hashing. Reuse the existing
    // exact OS-alias check; arbitrary links do not gain read authority.
    matches!(
        (std::path::absolute(installed), std::path::absolute(cached)),
        (Ok(installed), Ok(cached))
            if crate::materialize::normalize_system_alias(&installed)
                == crate::materialize::normalize_system_alias(&cached)
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn linked_mode_recognizes_only_verified_system_aliases() {
        for (alias, real) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
            assert_eq!(std::fs::canonicalize(alias).unwrap(), Path::new(real));
            // Structural path comparison; no files are created under /var or /tmp.
            let installed = Path::new(real).join("owned-comparison-cache");
            let cached = Path::new(alias).join("owned-comparison-cache");
            assert!(same_cached_install(&installed, &cached));
            assert!(same_cached_install(&cached, &installed));
            assert!(!same_cached_install(
                &installed,
                &Path::new(alias).join("different-cache")
            ));
        }
        assert!(!same_cached_install(
            Path::new("/private/variety/cache"),
            Path::new("/variety/cache")
        ));
        assert!(!same_cached_install(
            Path::new("/owned/actual-cache"),
            Path::new("/owned/user-alias")
        ));
    }
}
