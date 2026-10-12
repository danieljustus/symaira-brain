//! Skills-specific configkit precedence; malformed input resets the CLI to defaults.

use std::path::{Path, PathBuf};

use serde::Serialize;
use toml_edit::DocumentMut;

use crate::SkillError;

/// Resolved skills paths. `Targets` is deliberately null: the shipped CLI's
/// configkit loader ignores this untagged field and never calls `LoadTargets`.
#[derive(Debug, Clone, Serialize)]
pub struct Config {
    /// Portable library root.
    pub library_dir: PathBuf,
    /// Live rendered artifacts.
    pub render_dir: PathBuf,
    /// Comparison cache root.
    pub cache_dir: PathBuf,
    /// Global context-profile directory.
    pub profiles_dir: PathBuf,
    /// Frozen install bases.
    pub base_dir: PathBuf,
    /// Untagged Go field, not loaded by configkit.
    #[serde(rename = "Targets")]
    pub targets: Option<()>,
    /// Per-skill versioning policy.
    pub vcs: Vcs,
}

/// Versioning defaults on; an explicit false overrides it.
#[derive(Debug, Clone, Serialize)]
pub struct Vcs {
    /// Whether restore may create commits.
    pub enabled: bool,
}

/// Go's platform home variable, without fabricating a home when it is absent.
#[must_use]
pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Config remains in the legacy namespace; only absolute XDG overrides apply.
#[must_use]
pub fn config_path() -> PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"));
    root.join("symskills").join("config.toml")
}

/// Lifecycle log deliberately ignores XDG, matching events.DefaultPath.
#[must_use]
pub fn events_path() -> PathBuf {
    home_dir()
        .join(".local")
        .join("share")
        .join("symskills")
        .join("events.jsonl")
}

/// Default paths shared by CLI and the embedded MCP adapter.
#[must_use]
pub fn defaults() -> Config {
    let data = symbrain_core::paths::skills_data_dir()
        .map_or_else(|| PathBuf::from("."), |location| location.dir);
    let cache = symbrain_core::paths::skills_cache_dir()
        .map_or_else(|| PathBuf::from("."), |location| location.dir);
    Config {
        library_dir: data.join("library"),
        render_dir: data.join("rendered"),
        cache_dir: cache,
        profiles_dir: config_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("profiles"),
        base_dir: data.join("base"),
        targets: None,
        vcs: Vcs { enabled: true },
    }
}

/// CLI loadSkillsEnv discards every partial merge on any load error.
#[must_use]
pub fn load_cli() -> Config {
    load().unwrap_or_else(|_| defaults())
}

fn load() -> Result<Config, SkillError> {
    let mut config = defaults();
    merge_file(&mut config, &config_path())?;
    if let Ok(cwd) = std::env::current_dir() {
        merge_file(&mut config, &cwd.join(".symskills.toml"))?;
    }
    for (name, field) in [
        ("LIBRARY_DIR", &mut config.library_dir),
        ("RENDER_DIR", &mut config.render_dir),
        ("CACHE_DIR", &mut config.cache_dir),
        ("PROFILES_DIR", &mut config.profiles_dir),
        ("BASE_DIR", &mut config.base_dir),
    ] {
        if let Some(value) =
            std::env::var_os(format!("SYMSKILLS_{name}")).filter(|value| !value.is_empty())
        {
            *field = PathBuf::from(value);
        }
    }
    if let Ok(value) = std::env::var("SYMSKILLS_VCS_ENABLED")
        && !value.is_empty()
    {
        config.vcs.enabled = parse_bool(&value)?;
    }
    Ok(config)
}

fn parse_bool(value: &str) -> Result<bool, SkillError> {
    match value {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Ok(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Ok(false),
        _ => Err(SkillError("invalid vcs.enabled boolean".into())),
    }
}

fn merge_file(config: &mut Config, path: &Path) -> Result<(), SkillError> {
    if std::fs::metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let root = cap_std::fs::Dir::open_ambient_dir(parent, ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let filename = Path::new(
        path.file_name()
            .ok_or_else(|| SkillError("config filename missing".into()))?,
    );
    let bytes = crate::load::read_limited_nofollow(
        &root,
        filename,
        "skills config",
        crate::MAX_INPUT_SIZE,
    )?;
    let text = std::str::from_utf8(&bytes).map_err(|error| SkillError(error.to_string()))?;
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|error| SkillError(error.to_string()))?;
    for (name, field) in [
        ("library_dir", &mut config.library_dir),
        ("render_dir", &mut config.render_dir),
        ("cache_dir", &mut config.cache_dir),
        ("profiles_dir", &mut config.profiles_dir),
        ("base_dir", &mut config.base_dir),
    ] {
        if let Some(value) = doc.get(name) {
            // configkit skips zero false/numeric values before string conversion.
            if value.as_bool() == Some(false)
                || value.as_integer() == Some(0)
                || value.as_float() == Some(0.0)
            {
                continue;
            }
            let value = value
                .as_str()
                .ok_or_else(|| SkillError(format!("field {name} is not a string")))?;
            if !value.is_empty() {
                *field = PathBuf::from(value);
            }
        }
    }
    if let Some(enabled) = doc.get("vcs").and_then(|vcs| vcs.get("enabled")) {
        config.vcs.enabled = if let Some(value) = enabled.as_bool() {
            value
        } else if let Some(value) = enabled.as_str() {
            parse_bool(value)?
        } else {
            return Err(SkillError("vcs.enabled is not boolean".into()));
        };
    }
    Ok(())
}
