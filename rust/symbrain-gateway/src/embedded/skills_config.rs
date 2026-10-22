//! Go `config.Defaults()` paths for the embedded Skills tools.
//!
//! The gateway registers the tools with default options only (no config file,
//! no env override), so the library, base, render and profile directories are
//! the defaults of `internal/skills/config`.

use std::path::PathBuf;

pub(super) struct Dirs {
    pub(super) library: PathBuf,
    pub(super) render: PathBuf,
    pub(super) base: PathBuf,
    pub(super) profiles: PathBuf,
}

/// Go's platform home variable (`os.UserHomeDir`), empty when unset.
pub(super) fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// `configkit.DefaultPath("symskills")`: only an absolute XDG root applies.
fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("symskills")
        .join("config.toml")
}

pub(super) fn defaults() -> Dirs {
    let data = symbrain_core::paths::skills_data_dir()
        .map_or_else(|| PathBuf::from("."), |location| location.dir);
    Dirs {
        library: data.join("library"),
        render: data.join("rendered"),
        base: data.join("base"),
        profiles: config_path()
            .parent()
            .map_or_else(|| PathBuf::from("profiles"), |dir| dir.join("profiles")),
    }
}
