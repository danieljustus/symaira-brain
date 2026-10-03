//! Native memory paths contracts.

use super::{PathBuf, Store, Write, exit};

/// Resolves the memory database the native commands open.
///
/// Mirrors the shipped implementation: the configured/`--db` override wins,
/// then the current `<data>/memory/default.db`, then a legacy
/// `~/.local/share/symmemory` installation. `SYMBRAIN_MEMORY_DB_PATH` is a
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
    let current = symbrain_core::xdg::data_dir().map(|dir| dir.join("memory"));
    let legacy = symbrain_core::xdg::home_dir().map(|home| home.join(".local/share/symmemory"));
    let directory = match (&current, &legacy) {
        (Some(current), Some(legacy)) if !current.exists() && legacy.exists() => legacy.clone(),
        (Some(current), _) => current.clone(),
        (None, Some(legacy)) => legacy.clone(),
        (None, None) => PathBuf::from(".memory"),
    };
    directory.join("default.db")
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
