//! Recovery for retained profile logs that fail chain verification (#1019).
//!
//! Go-era logs restarted the hash chain in every process, so the Rust sink's
//! fail-closed recovery rejects them and the gateway would run without an
//! audit log. The unverifiable files are kept next to the log under an
//! `.unverified-<utc>` suffix (ignored by `audit tail`) and a new chain starts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::logger::Logger;
use crate::model::Config;

/// Files of one profile log: the current segment, the rotated segment and
/// its anchors (see `sink.rs`).
const SEGMENT_SUFFIXES: [&str; 4] = ["", ".1", ".1.anchor", ".1.anchor.tmp"];

impl Logger {
    /// Opens the profile log under the process XDG audit directory, preserving
    /// an unverifiable retained log instead of failing.
    ///
    /// Returns the logger and, when a log was set aside, the preserved path of
    /// its current segment.
    ///
    /// # Errors
    /// Returns an error when the audit directory cannot be resolved, a file
    /// cannot be moved aside, or the fresh log cannot be opened.
    pub fn open_preserving_unverified(
        profile: &str,
        config: Config,
    ) -> io::Result<(Self, Option<PathBuf>)> {
        let dir = symbrain_core::xdg::audit_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "audit: resolve audit dir: home directory unavailable",
            )
        })?;
        Self::open_in_preserving_unverified(&dir, profile, config)
    }

    /// [`Logger::open_preserving_unverified`] in an explicit directory.
    ///
    /// # Errors
    /// See [`Logger::open_preserving_unverified`].
    pub fn open_in_preserving_unverified(
        dir: &Path,
        profile: &str,
        config: Config,
    ) -> io::Result<(Self, Option<PathBuf>)> {
        match Self::open_in(dir, profile, config) {
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let preserved = set_aside(dir, profile)?;
                Ok((Self::open_in(dir, profile, config)?, preserved))
            }
            other => other.map(|logger| (logger, None)),
        }
    }
}

fn set_aside(dir: &Path, profile: &str) -> io::Result<Option<PathBuf>> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let base = format!("{profile}.jsonl");
    let mut preserved = None;
    for suffix in SEGMENT_SUFFIXES {
        let from = dir.join(format!("{base}{suffix}"));
        if !from.exists() {
            continue;
        }
        let to = dir.join(format!("{base}{suffix}.unverified-{stamp}"));
        fs::rename(&from, &to)?;
        if suffix.is_empty() {
            preserved = Some(to);
        }
    }
    Ok(preserved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const ENABLED: Config = Config {
        enabled: true,
        verbose: false,
    };

    #[test]
    fn go_era_log_is_preserved_and_a_new_chain_starts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("personal.jsonl");
        let legacy = "{\"timestamp\":\"2026-08-25T12:00:49Z\",\"status\":\"degraded\"}\n";
        fs::write(&path, legacy).unwrap();
        assert!(Logger::open_in(dir.path(), "personal", ENABLED).is_err());

        let (logger, preserved) =
            Logger::open_in_preserving_unverified(dir.path(), "personal", ENABLED).unwrap();
        let preserved = preserved.expect("legacy log set aside");
        assert_eq!(fs::read_to_string(preserved).unwrap(), legacy);

        logger.log_degradation("vault", "probe", "warning");
        logger.close().unwrap();
        assert!(Logger::open_in(dir.path(), "personal", ENABLED).is_ok());
    }

    #[test]
    fn verifiable_log_is_left_in_place() {
        let dir = tempdir().unwrap();
        let (logger, preserved) =
            Logger::open_in_preserving_unverified(dir.path(), "personal", ENABLED).unwrap();
        assert!(preserved.is_none());
        logger.log_degradation("vault", "probe", "warning");
        logger.close().unwrap();
        let (_, preserved) =
            Logger::open_in_preserving_unverified(dir.path(), "personal", ENABLED).unwrap();
        assert!(preserved.is_none());
    }
}
