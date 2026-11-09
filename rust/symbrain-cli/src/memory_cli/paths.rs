//! Native memory paths contracts.

use super::{PathBuf, Store, Write, exit};

/// Resolves the memory database the native commands open.
///
/// Mirrors the shipped implementation: the configured/`--db` override wins,
/// then the current `<data>/symbrain/memory/default.db`, then a legacy
/// `<data>/symmemory` installation. Relative XDG data roots are ignored.
/// `SYMBRAIN_MEMORY_DB_PATH` is a
/// Rust-side test hook and has no counterpart in the shipped CLI.
pub(crate) fn resolve_db_path(override_path: Option<&std::ffi::OsStr>) -> PathBuf {
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
    let home = std::env::var_os("HOME")
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
    // A current directory without a database (created by an older gateway
    // start, #1017) must not shadow a legacy store that holds the data.
    let shadowed = legacy.join("default.db").is_file() && !current.join("default.db").is_file();
    Some(if shadowed || (!current.is_dir() && legacy.is_dir()) {
        legacy
    } else {
        current
    })
}

/// Opens (creating when absent) the store the way the shipped commands do;
/// every failure keeps the shipped `<verb>: open memory database:` diagnostic.
pub(super) fn open_store(
    stderr: &mut dyn Write,
    verb: &str,
    override_path: Option<&std::ffi::OsStr>,
) -> Result<Store, u8> {
    let db_path = resolve_db_path(override_path);
    Store::open(&db_path).map_err(|err| {
        let _ = writeln!(
            stderr,
            "symbrain memory {verb}: open memory database: {err}"
        );
        exit::GENERIC
    })
}

#[cfg(test)]
mod tests {
    use super::data_directory;

    #[test]
    fn data_directory_prefers_the_store_that_holds_data() {
        // (current dir, current db, legacy dir, legacy db) -> legacy chosen
        let cases = [
            ((false, false, false, false), false),
            ((true, false, false, false), false),
            ((false, false, true, false), true),
            ((false, false, true, true), true),
            ((true, true, true, true), false),
            ((true, false, true, false), false),
            // #1017: an empty current dir must not shadow legacy data.
            ((true, false, true, true), true),
        ];
        for ((cur_dir, cur_db, leg_dir, leg_db), want_legacy) in cases {
            let home = tempfile::tempdir().expect("tempdir");
            let base = home.path().join(".local/share");
            let current = base.join("symbrain/memory");
            let legacy = base.join("symmemory");
            for (dir, db, path) in [(cur_dir, cur_db, &current), (leg_dir, leg_db, &legacy)] {
                if dir {
                    std::fs::create_dir_all(path).expect("dir");
                }
                if db {
                    std::fs::write(path.join("default.db"), b"").expect("db");
                }
            }
            let got = data_directory(None, Some(home.path())).expect("resolves");
            let want = if want_legacy { &legacy } else { &current };
            assert_eq!(&got, want, "case {cur_dir} {cur_db} {leg_dir} {leg_db}");
        }
    }
}
