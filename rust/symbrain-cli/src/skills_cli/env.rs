//! Go `loadSkillsEnv`: the symskills configuration plus the home, project and
//! log locations every `symbrain skills` subcommand works against.
//!
//! Nothing here can fail. Go degrades every failure the same way — an
//! undefined home leaves `homeDir` empty, an unreadable working directory
//! leaves `projectDir` empty and any `config.Load()` error (a config file that
//! cannot be stat'ed, read, parsed or applied, an invalid
//! `SYMSKILLS_VCS_ENABLED`, no home at all) replaces the **whole** config by
//! `config.Defaults()`, environment overrides included — and the native
//! replay does the same.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use symbrain_skills::config_toml::{self, ConfigFile, LoadOutcome, SkillsConfig};
use symbrain_skills::gotext::{clean, join};

/// Go's `config.Config` fields the skills commands read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Config {
    pub(super) library_dir: PathBuf,
    pub(super) render_dir: PathBuf,
    pub(super) cache_dir: PathBuf,
    pub(super) profiles_dir: PathBuf,
    pub(super) base_dir: PathBuf,
    pub(super) vcs_enabled: bool,
}

/// Go's `skillsEnv`.
#[derive(Clone, Debug)]
pub(super) struct Env {
    pub(super) cfg: Config,
    /// `os.UserHomeDir()`, empty when Go reports an error.
    pub(super) home_dir: PathBuf,
    /// `os.Getwd()`, empty when Go reports an error.
    pub(super) project_dir: PathBuf,
    /// `events.DefaultPath()`.
    pub(super) log_path: PathBuf,
    /// `config.ConfigPath()`.
    pub(super) config_path: PathBuf,
    /// A config file was merged, or any `SYMSKILLS_*` variable is set.
    pub(super) customised: bool,
}

/// `os.UserHomeDir()` on Unix: a non-empty `$HOME`.
pub(super) fn go_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `os.Getwd()`: an absolute `$PWD` that names the working directory wins
/// over the physical path.
pub(super) fn go_getwd() -> Option<PathBuf> {
    let physical = std::env::current_dir().ok();
    if let Some(pwd) = std::env::var_os("PWD").map(PathBuf::from)
        && pwd.is_absolute()
        && same_directory(&pwd, Path::new("."))
    {
        return Some(pwd);
    }
    physical
}

fn same_directory(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

/// `configkit.DefaultPath("symskills")`.
pub(super) fn config_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        && xdg.is_absolute()
    {
        return join(xdg, "symskills/config.toml");
    }
    match go_home() {
        Some(home) => join(home, ".config/symskills/config.toml"),
        None => PathBuf::from(".config/symskills/config.toml"),
    }
}

/// `events.DefaultPath()`.
pub(super) fn log_path() -> PathBuf {
    join(
        go_home().unwrap_or_else(|| PathBuf::from(".")),
        ".local/share/symskills/events.jsonl",
    )
}

/// `config.Defaults()`.
pub(super) fn defaults() -> Config {
    let data = symbrain_skills::runner::skills_data_root()
        .map_or_else(|_| PathBuf::from("."), |root| clean(&root));
    let cache = symbrain_core::paths::skills_cache_dir()
        .map_or_else(|| PathBuf::from("."), |location| clean(&location.dir));
    let config_dir = config_path()
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Config {
        library_dir: join(&data, "library"),
        render_dir: join(&data, "rendered"),
        cache_dir: cache,
        profiles_dir: join(config_dir, "profiles"),
        base_dir: join(&data, "base"),
        vcs_enabled: true,
    }
}

/// Go `strconv.ParseBool`.
fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

fn to_skills(config: &Config) -> SkillsConfig {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    SkillsConfig {
        library_dir: text(&config.library_dir),
        render_dir: text(&config.render_dir),
        cache_dir: text(&config.cache_dir),
        profiles_dir: text(&config.profiles_dir),
        base_dir: text(&config.base_dir),
        vcs_enabled: config.vcs_enabled,
    }
}

/// Merges a decoded file result onto `before`; a field the file left alone
/// keeps its original (possibly non-UTF-8) bytes.
fn from_skills(before: &Config, loaded: SkillsConfig) -> Config {
    let pick = |old: &Path, new: String| {
        if new == old.to_string_lossy() {
            old.to_path_buf()
        } else {
            PathBuf::from(new)
        }
    };
    Config {
        library_dir: pick(&before.library_dir, loaded.library_dir),
        render_dir: pick(&before.render_dir, loaded.render_dir),
        cache_dir: pick(&before.cache_dir, loaded.cache_dir),
        profiles_dir: pick(&before.profiles_dir, loaded.profiles_dir),
        base_dir: pick(&before.base_dir, loaded.base_dir),
        vcs_enabled: loaded.vcs_enabled,
    }
}

/// `configkit.mergeFile` for one source: absent files are skipped, any other
/// stat, read, parse or apply failure is a `Load` error.
///
/// A document the decoder hands back (`NeedsGo`) is a document `BurntSushi`
/// either rejects or decodes through a grammar quirk `toml_edit` lacks; the
/// replay treats it as a parse failure, which `Load` reports as an error.
fn merge_file(config: &Config, path: &Path) -> Result<(Config, bool), ()> {
    if matches!(fs::metadata(path), Err(error) if error.raw_os_error() == Some(2)) {
        return Ok((config.clone(), false));
    }
    let content = fs::read(path).map_err(|_| ())?;
    let display = path.to_string_lossy();
    match config_toml::load(
        &to_skills(config),
        Some(ConfigFile {
            path: &display,
            content: &content,
        }),
        None,
    ) {
        LoadOutcome::Loaded(loaded) => Ok((from_skills(config, loaded), true)),
        LoadOutcome::Error(_) | LoadOutcome::Rejected | LoadOutcome::NeedsGo(_) => Err(()),
    }
}

/// `SYMSKILLS_*` overrides (`applyEnvOverrides`): a variable that is set
/// overrides, even when empty; an unparsable bool is an error.
fn apply_env(mut config: Config) -> Result<Config, ()> {
    let slots: [(&str, &mut PathBuf); 5] = [
        ("SYMSKILLS_LIBRARY_DIR", &mut config.library_dir),
        ("SYMSKILLS_RENDER_DIR", &mut config.render_dir),
        ("SYMSKILLS_CACHE_DIR", &mut config.cache_dir),
        ("SYMSKILLS_PROFILES_DIR", &mut config.profiles_dir),
        ("SYMSKILLS_BASE_DIR", &mut config.base_dir),
    ];
    for (name, slot) in slots {
        if let Some(value) = std::env::var_os(name) {
            *slot = PathBuf::from(value);
        }
    }
    if let Some(value) = std::env::var_os("SYMSKILLS_VCS_ENABLED") {
        config.vcs_enabled = value.to_str().and_then(parse_bool).ok_or(())?;
    }
    Ok(config)
}

/// `config.Load()` and the swap to `Defaults()` on error.
fn load_config(defaults: &Config, project_dir: Option<&Path>) -> Config {
    let attempt = || -> Result<Config, ()> {
        // `configkit.loadOnce` fails before reading anything without a home.
        go_home().ok_or(())?;
        let (config, _) = merge_file(defaults, &config_path())?;
        let config = match project_dir {
            Some(dir) => merge_file(&config, &join(dir, ".symskills.toml"))?.0,
            None => config,
        };
        apply_env(config)
    };
    attempt().unwrap_or_else(|()| defaults.clone())
}

/// Whether a `SYMSKILLS_*` variable is set or a config source is anything but
/// verifiably absent. Go's bytes carry no #621 render extension then.
fn has_custom_sources(project_dir: Option<&Path>) -> bool {
    let absent =
        |path: &Path| matches!(fs::metadata(path), Err(error) if error.raw_os_error() == Some(2));
    std::env::vars_os().any(|(name, _)| is_symskills_variable(&name))
        || !absent(&config_path())
        || project_dir.is_some_and(|dir| !absent(&join(dir, ".symskills.toml")))
}

fn is_symskills_variable(name: &OsString) -> bool {
    matches!(
        name.to_str(),
        Some(
            "SYMSKILLS_LIBRARY_DIR"
                | "SYMSKILLS_RENDER_DIR"
                | "SYMSKILLS_CACHE_DIR"
                | "SYMSKILLS_PROFILES_DIR"
                | "SYMSKILLS_BASE_DIR"
                | "SYMSKILLS_VCS_ENABLED"
        )
    )
}

impl Env {
    /// Go `loadSkillsEnv()`.
    pub(super) fn load() -> Self {
        let project = go_getwd();
        let cfg = load_config(&defaults(), project.as_deref());
        Self {
            cfg,
            customised: has_custom_sources(project.as_deref()),
            // Go's `filepath.Join` cleans every derived path lexically, so the
            // home is cleaned once instead of at each join.
            home_dir: go_home().map(|home| clean(&home)).unwrap_or_default(),
            project_dir: project.unwrap_or_default(),
            log_path: log_path(),
            config_path: config_path(),
        }
    }
}
