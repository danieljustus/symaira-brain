//! Go-exact symskills configuration for `symbrain skills doctor` (SKL-015).
//!
//! Go's `cmdSkillsDoctor` prints the merged symskills config, so unlike
//! `skills targets` the report depends on it. `loadSkillsEnv` calls
//! `config.Load()` — `configkit.Loader.loadOnce`: `Defaults()`, then the global
//! `config.toml`, then `./.symskills.toml`, then `SYMSKILLS_*` variables — and
//! replaces the **whole** result with `config.Defaults()` when `Load` returns
//! any error, environment overrides included.
//!
//! Reconstructable without a TOML decoder:
//!
//! * both config files absent (`os.Stat` reports `ENOENT`): defaults, then the
//!   environment. `SYMSKILLS_{LIBRARY,RENDER,CACHE,PROFILES,BASE}_DIR` replace
//!   the field verbatim (an empty value included, no path cleaning);
//!   `SYMSKILLS_VCS_ENABLED` goes through `strconv.ParseBool` and an invalid
//!   spelling makes `Load` fail, which resets the config to the defaults;
//! * a config path whose stat fails with anything but `ENOENT` (`ENOTDIR`,
//!   `EACCES`, `ELOOP`, ...) or that is a directory: `toml.DecodeFile` fails,
//!   so the whole config resets to the defaults whatever the environment says.
//!   The global file is merged first, so its failure ends the load; a project
//!   failure resets regardless of the global file's content (a decode error
//!   there resets as well).
//!
//! Any config file with content (or a special file) needs the Go TOML decoder
//! and stays on Go, as does every shape whose bytes native code could not
//! print exactly: unclean or non-UTF-8 base paths (Go's `filepath.Join`
//! cleans, `PathBuf::join` does not), a `$PWD` that is not the physical
//! working directory (Go's `os.Getwd` returns `$PWD`), and U+2028/U+2029
//! (Go's JSON encoder escapes them, `serde_json` does not).

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::{is_go_not_exist, is_lexically_clean, skills_config_path, windows_path_needs_go};

/// The merged config `skills doctor` prints, as Go's `config.Config` fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DoctorConfig {
    pub(super) library_dir: String,
    pub(super) render_dir: String,
    pub(super) cache_dir: String,
    pub(super) profiles_dir: String,
    pub(super) base_dir: String,
    pub(super) vcs_enabled: bool,
}

/// Go `strconv.ParseBool`: the only spellings `SYMSKILLS_VCS_ENABLED` accepts.
fn go_bool(value: &OsStr) -> Option<bool> {
    match value.to_str()? {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// Text Go's JSON encoder and `serde_json` write identically: Go escapes
/// U+2028/U+2029 as `\u2028`/`\u2029`, `serde_json` writes them raw.
fn json_plain(text: &str) -> bool {
    !text.contains(['\u{2028}', '\u{2029}'])
}

/// An absolute, UTF-8, `filepath.Clean`-form base directory that
/// `PathBuf::join` extends to exactly the bytes `filepath.Join` produces.
fn plain_base(value: &OsStr) -> bool {
    let Some(text) = value.to_str() else {
        return false;
    };
    Path::new(text).is_absolute()
        && is_lexically_clean(text)
        && json_plain(text)
        && !(cfg!(windows) && (windows_path_needs_go(text) || text.contains('/')))
}

/// An XDG base variable Go ignores (unset, empty or relative) or one that is
/// a plain absolute base.
fn xdg_base_plain(name: &str) -> bool {
    match std::env::var_os(name) {
        None => true,
        Some(value) if !Path::new(&value).is_absolute() => true,
        Some(value) => plain_base(&value),
    }
}

/// Go's `os.Getwd` returns an absolute `$PWD` that names the working
/// directory (same device and inode as `.`) instead of the physical path, so a
/// `$PWD` spelled differently from `cwd` that still names it would change
/// `project_dir`; those stay on Go. A relative, unreachable or unrelated
/// `$PWD` is ignored by Go exactly as here.
fn pwd_is_physical(cwd: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let Some(pwd) = std::env::var_os("PWD") else {
            return true;
        };
        let pwd = Path::new(&pwd);
        if !pwd.is_absolute() || pwd == cwd {
            return true;
        }
        match (fs::metadata(pwd), fs::metadata(cwd)) {
            (Ok(named), Ok(dot)) => named.dev() != dot.dev() || named.ino() != dot.ino(),
            (_, Err(_)) => false,
            (Err(_), Ok(_)) => true,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = cwd;
        true
    }
}

/// How `configkit.mergeFile` sees one config source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Probe {
    /// `os.Stat` reports `ENOENT`: the source is skipped.
    Absent,
    /// The probe or the read fails, so `config.Load()` returns an error.
    Fails,
    /// A regular file whose TOML only Go decodes.
    Regular,
    /// Anything else (FIFO, socket, device, non-Unix error shapes).
    Other,
}

fn probe(path: &Path) -> Probe {
    match fs::metadata(path) {
        Err(error) if is_go_not_exist(&error) => Probe::Absent,
        // Stat fails with ENOTDIR/EACCES/ELOOP/...: `os.IsNotExist` is false,
        // so Go opens the path and the open fails the same way.
        Err(_) if cfg!(unix) => Probe::Fails,
        // `toml.DecodeFile` reads the directory and fails (EISDIR on Unix,
        // "Incorrect function" on Windows; CI run 40215584258 shows Go on
        // Windows resetting to the defaults for these shapes too).
        Ok(metadata) if metadata.is_dir() => Probe::Fails,
        Ok(metadata) if metadata.is_file() => Probe::Regular,
        _ => Probe::Other,
    }
}

/// Whether `config.Load()` fails (so the config resets to the defaults), or
/// `None` when the outcome needs Go's TOML decoder.
fn load_fails(global: Probe, project: Probe) -> Option<bool> {
    match (global, project) {
        (Probe::Absent, Probe::Absent) => Some(false),
        // The global file is merged first; its failure ends the load.
        (Probe::Fails, _) | (Probe::Absent | Probe::Regular, Probe::Fails) => Some(true),
        _ => None,
    }
}

/// The package defaults: `config.Defaults()` with the data and cache roots
/// from the shared resolver and the profiles directory beside `config.toml`.
fn defaults(config_path: &Path) -> Option<DoctorConfig> {
    let data_root = symbrain_core::paths::skills_data_dir()?.dir;
    let cache_root = symbrain_core::paths::skills_cache_dir()?.dir;
    let profiles = config_path.parent()?.join("profiles");
    Some(DoctorConfig {
        library_dir: data_root.join("library").display().to_string(),
        render_dir: data_root.join("rendered").display().to_string(),
        cache_dir: cache_root.display().to_string(),
        profiles_dir: profiles.display().to_string(),
        base_dir: data_root.join("base").display().to_string(),
        vcs_enabled: true,
    })
}

/// Applies the `SYMSKILLS_*` environment as `configkit.applyEnvOverrides`
/// does, or returns the defaults when Go's `Load` would fail on it.
fn apply_env(defaults: DoctorConfig) -> Option<DoctorConfig> {
    let mut config = defaults.clone();
    if let Some(value) = std::env::var_os("SYMSKILLS_VCS_ENABLED") {
        let Some(enabled) = go_bool(&value) else {
            return Some(defaults);
        };
        config.vcs_enabled = enabled;
    }
    for (name, slot) in [
        ("SYMSKILLS_LIBRARY_DIR", &mut config.library_dir),
        ("SYMSKILLS_RENDER_DIR", &mut config.render_dir),
        ("SYMSKILLS_CACHE_DIR", &mut config.cache_dir),
        ("SYMSKILLS_PROFILES_DIR", &mut config.profiles_dir),
        ("SYMSKILLS_BASE_DIR", &mut config.base_dir),
    ] {
        if let Some(value) = std::env::var_os(name) {
            let text = value.to_str()?;
            if !json_plain(text) {
                return None;
            }
            text.clone_into(slot);
        }
    }
    Some(config)
}

/// The config `skills doctor` prints, or `None` when only Go can produce it.
pub(super) fn resolve() -> Option<DoctorConfig> {
    // An undefined home is the caller's gate (`go_home_defined`): Go's
    // `os.UserHomeDir` fails there and the roots are built against "".
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    if (!home.is_empty() && !plain_base(&home))
        || !["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]
            .iter()
            .all(|name| xdg_base_plain(name))
    {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    if !cwd.to_str().is_some_and(json_plain) || !pwd_is_physical(&cwd) {
        return None;
    }
    let config_path = skills_config_path();
    let defaults = defaults(&config_path)?;
    let global = probe(&config_path);
    let project = probe(&cwd.join(".symskills.toml"));
    if load_fails(global, project)? {
        return Some(defaults);
    }
    apply_env(defaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_bool_accepts_exactly_the_strconv_spellings() {
        for yes in ["1", "t", "T", "TRUE", "true", "True"] {
            assert_eq!(go_bool(OsStr::new(yes)), Some(true), "{yes}");
        }
        for no in ["0", "f", "F", "FALSE", "false", "False"] {
            assert_eq!(go_bool(OsStr::new(no)), Some(false), "{no}");
        }
        for bad in ["", "yes", "no", "tRuE", "TRUe", " true", "true ", "2", "on"] {
            assert_eq!(go_bool(OsStr::new(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn json_plain_rejects_only_the_unescaped_separators() {
        assert!(json_plain("/a\nb\u{1}\t\"\\<>&"));
        assert!(!json_plain("/a\u{2028}b"));
        assert!(!json_plain("/a\u{2029}b"));
    }

    #[cfg(unix)]
    #[test]
    fn plain_base_requires_absolute_clean_utf8() {
        use std::os::unix::ffi::OsStrExt;

        for plain in ["/", "/a", "/a/b.c", "/a/.hidden"] {
            assert!(plain_base(OsStr::new(plain)), "{plain}");
        }
        for unplain in [
            "",
            "a",
            "./a",
            "/a/",
            "/a//b",
            "/a/./b",
            "/a/../b",
            "/a\u{2028}",
        ] {
            assert!(!plain_base(OsStr::new(unplain)), "{unplain:?}");
        }
        assert!(!plain_base(OsStr::from_bytes(b"/a\xffb")));
    }

    #[test]
    fn load_failure_table_follows_the_merge_order() {
        use Probe::{Absent, Fails, Other, Regular};
        assert_eq!(load_fails(Absent, Absent), Some(false));
        assert_eq!(load_fails(Fails, Absent), Some(true));
        assert_eq!(load_fails(Fails, Regular), Some(true));
        assert_eq!(load_fails(Fails, Other), Some(true));
        assert_eq!(load_fails(Absent, Fails), Some(true));
        assert_eq!(load_fails(Regular, Fails), Some(true));
        assert_eq!(load_fails(Regular, Absent), None);
        assert_eq!(load_fails(Absent, Regular), None);
        assert_eq!(load_fails(Regular, Regular), None);
        assert_eq!(load_fails(Other, Fails), None);
        assert_eq!(load_fails(Absent, Other), None);
    }

    #[test]
    fn probe_directory_fails_and_missing_is_absent_on_every_platform() {
        // Go's `toml.DecodeFile` fails on a directory on Windows as well
        // (SKL-015 CI run 40215584258), so the doctor must stay native there.
        let root = tempfile::TempDir::new().unwrap();
        let dir = root.path().join("dir");
        fs::create_dir(&dir).unwrap();
        assert_eq!(probe(&dir), Probe::Fails);
        assert_eq!(probe(&root.path().join("missing")), Probe::Absent);
    }

    #[cfg(unix)]
    #[test]
    fn probe_classifies_stat_outcomes_like_go() {
        let root = tempfile::TempDir::new().unwrap();
        let dir = root.path().join("dir");
        fs::create_dir(&dir).unwrap();
        let file = root.path().join("file");
        fs::write(&file, b"x").unwrap();
        assert_eq!(probe(&root.path().join("missing")), Probe::Absent);
        assert_eq!(probe(&dir), Probe::Fails);
        assert_eq!(probe(&file), Probe::Regular);
        assert_eq!(probe(&file.join("below")), Probe::Fails);
        let looped = root.path().join("loop");
        std::os::unix::fs::symlink("loop", &looped).unwrap();
        assert_eq!(probe(&looped), Probe::Fails);
        let dangling = root.path().join("dangling");
        std::os::unix::fs::symlink("nowhere", &dangling).unwrap();
        assert_eq!(probe(&dangling), Probe::Absent);
    }
}
