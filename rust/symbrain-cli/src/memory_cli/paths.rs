//! Native memory paths contracts.

use super::{PathBuf, Store, Write, exit};

/// Resolves the memory database the native commands open.
///
/// Mirrors the shipped implementation: the configured/`--db` override wins,
/// then the current `<data>/symbrain/memory/default.db`, then a legacy
/// `<data>/symmemory` installation. Relative XDG data roots are ignored.
/// `SYMBRAIN_MEMORY_DB_PATH` is a
/// Rust-side test hook and has no counterpart in the shipped CLI.
pub(super) fn resolve_db_path(override_path: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let config = super::config::load();
    if !config.database.is_empty() {
        return PathBuf::from(&config.database);
    }
    if let Some(path) = std::env::var_os("SYMBRAIN_MEMORY_DB_PATH") {
        return PathBuf::from(path);
    }
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let directory =
        data_directory(xdg.as_deref(), home.as_deref()).unwrap_or_else(|| PathBuf::from(".memory"));
    directory.join("default.db")
}

fn data_directory(
    xdg: Option<&std::path::Path>,
    home: Option<&std::path::Path>,
) -> Option<PathBuf> {
    // Frozen internal/paths is stricter than the general core data resolver:
    // both namespace candidates share one absolute-XDG-or-HOME base.
    let base = xdg.filter(|path| path.is_absolute()).map_or_else(
        || home.map(|home| home.join(".local/share")),
        |path| Some(path.to_path_buf()),
    )?;
    let current = base.join("symbrain/memory");
    let legacy = base.join("symmemory");
    Some(if current.is_dir() || !legacy.is_dir() {
        current
    } else {
        legacy
    })
}

pub(super) fn open_store(
    stderr: &mut dyn Write,
    override_path: Option<&std::ffi::OsStr>,
) -> Result<Store, u8> {
    let db_path = resolve_db_path(override_path);
    Store::open(&db_path).map_err(|err| {
        let _ = writeln!(stderr, "symbrain memory: open database: {err}");
        exit::GENERIC
    })
}
