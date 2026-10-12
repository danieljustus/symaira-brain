fn build_harness_map() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for harness in symbrain_harness::all() {
        if let Some(target_name) = skill_target_name(harness.skill_target) {
            map.insert(harness.name.as_str().to_string(), target_name.to_string());
        }
    }
    map
}

/// Returns the skill target name for a given `SkillTarget` variant, or None.
fn skill_target_name(target: symbrain_harness::SkillTarget) -> Option<&'static str> {
    match target {
        symbrain_harness::SkillTarget::None => None,
        symbrain_harness::SkillTarget::OpenCode => Some("opencode"),
        symbrain_harness::SkillTarget::Claude => Some("claude"),
        symbrain_harness::SkillTarget::Codex => Some("codex"),
        symbrain_harness::SkillTarget::Hermes => Some("hermes"),
        symbrain_harness::SkillTarget::Antigravity => Some("antigravity"),
        symbrain_harness::SkillTarget::OpenClaw => Some("openclaw"),
    }
}

/// Context trait for timeout/cancellation checking.
pub trait Context {
    /// Returns true if the operation should be cancelled.
    fn is_cancelled(&self) -> bool;
}

/// No-op context for use without actual timeout/cancellation.
#[derive(Default)]
pub struct NoopContext;

impl Context for NoopContext {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Resolves the skills data root the way Go's `sharedpaths.SkillsDataDir()`
/// does through `internal/paths.resolve`: `$XDG_DATA_HOME` when it holds an
/// absolute path (a relative value is ignored per the XDG spec), else
/// `os.UserHomeDir()/.local/share`; then `base/symbrain/skills`, unless that directory is
/// absent while the legacy `base/symskills` exists, in which case the legacy
/// directory wins.
///
/// Public so every native skills path (the CLI's library/base/render roots
/// included) shares this one resolver instead of re-deriving the precedence;
/// the branches are frozen by the `defaults` scenarios in
/// `tests/fixtures/runner_oracle.json`.
///
/// # Errors
///
/// Returns the same error text Go's `os.UserHomeDir` produces when no
/// absolute `$XDG_DATA_HOME` and no usable home directory are available.
pub fn skills_data_root() -> Result<PathBuf, SkillError> {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
        _ => home_dir()?.join(".local/share"),
    };
    let current = base.join("symbrain").join("skills");
    if current.is_dir() {
        return Ok(current);
    }
    let legacy = base.join("symskills");
    if legacy.is_dir() {
        return Ok(legacy);
    }
    Ok(current)
}

/// Reads the platform home variable with Go's `os.UserHomeDir` error text.
fn home_dir() -> Result<PathBuf, SkillError> {
    let (name, error) = if cfg!(windows) {
        ("USERPROFILE", "%userprofile% is not defined")
    } else {
        ("HOME", "$HOME is not defined")
    };
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| SkillError(error.into()))
}

/// Stringifies a resolved default path.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Default library directory: `config.Defaults()` uses `dataRoot/library`.
///
/// # Errors
///
/// Returns an error when the home directory cannot be resolved.
fn default_library_dir() -> Result<String, SkillError> {
    Ok(path_string(&skills_data_root()?.join("library")))
}

/// Default render directory: `config.Defaults()` uses `dataRoot/rendered`.
///
/// This is the data root, not `SkillsCacheDir()`: Go's `RenderDir` is the live
/// artifact a symlink install points at.
///
/// # Errors
///
/// Returns an error when the home directory cannot be resolved.
fn default_render_dir() -> Result<String, SkillError> {
    Ok(path_string(&skills_data_root()?.join("rendered")))
}

/// Default base directory: `config.Defaults()` uses `dataRoot/base`.
///
/// # Errors
///
/// Returns an error when the home directory cannot be resolved.
fn default_base_dir() -> Result<String, SkillError> {
    Ok(path_string(&skills_data_root()?.join("base")))
}
