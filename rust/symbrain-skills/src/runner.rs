//! Orchestration of skill rendering and installation across harness targets.

use crate::install;
use crate::load::load_bundle;
use crate::model::{Bundle, SkillError};
use crate::render;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Default per-target timeout matching the Go implementation.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Options for the skills runner.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Skills library directory.
    pub library_dir: String,
    /// Render output directory.
    pub render_dir: String,
    /// Base snapshot directory.
    pub base_dir: String,
    /// User home directory.
    pub home_dir: String,
    /// Project directory (optional).
    pub project_dir: String,
    /// Per-target timeout.
    pub timeout: Duration,
}

impl Options {
    /// Fills empty option fields from the default paths.
    ///
    /// # Errors
    ///
    /// Returns an error if the user home directory cannot be determined.
    #[allow(clippy::too_many_lines)]
    pub fn with_defaults(self) -> Result<Self, SkillError> {
        let mut opts = self;

        if opts.library_dir.is_empty() {
            opts.library_dir = default_library_dir()?;
        }
        if opts.render_dir.is_empty() {
            opts.render_dir = default_render_dir()?;
        }
        if opts.base_dir.is_empty() {
            opts.base_dir = default_base_dir()?;
        }
        if opts.home_dir.is_empty() {
            // Go wraps os.UserHomeDir's error verbatim, whose text on Unix is
            // "$HOME is not defined".
            opts.home_dir = std::env::var("HOME")
                .map_err(|_| SkillError("resolve home directory: $HOME is not defined".into()))?;
        }
        if opts.timeout == Duration::ZERO {
            opts.timeout = DEFAULT_TIMEOUT;
        }

        Ok(opts)
    }
}

/// Result for a single harness target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetResult {
    /// Harness target name.
    pub target: String,
    /// Status: "ok", "skipped", "error".
    pub status: String,
    /// Optional message.
    pub message: Option<String>,
}

/// Runs the skills pipeline for the given harness names.
///
/// For each harness, resolves its skill target and processes every skill
/// in the library through the render/install pipeline (or dry-run).
///
/// # Errors
///
/// Returns an error if the options cannot be finalized (e.g., home directory
/// resolution fails).
#[allow(clippy::too_many_lines)]
pub fn run(
    ctx: &mut impl Context,
    harness_names: &[String],
    mut opts: Options,
    dry_run: bool,
) -> Result<Vec<TargetResult>, SkillError> {
    opts = opts.with_defaults()?;

    // Build harness -> target map from the canonical registry
    let harness_map = build_harness_map();

    let mut results = Vec::with_capacity(harness_names.len());
    for harness in harness_names {
        let target_name = if let Some(t) = harness_map.get(harness) {
            t.clone()
        } else {
            results.push(TargetResult {
                target: harness.clone(),
                status: "skipped".to_string(),
                message: Some(format!("no skill target for harness {harness:?}")),
            });
            continue;
        };

        // Process target (timeout handled per-target inside sync_target, matching Go's WithTimeout)
        let result = sync_target(ctx, &target_name, &opts, dry_run);
        results.push(result);
    }

    Ok(results)
}

/// Synchronizes all skills for a single harness target.
///
/// Never panics: success, timeout and per-skill failures all travel through the
/// returned [`TargetResult`], matching Go's `syncTarget`, which likewise has no
/// error result of its own.
#[allow(clippy::too_many_lines)]
fn sync_target(
    ctx: &mut impl Context,
    target_name: &str,
    opts: &Options,
    dry_run: bool,
) -> TargetResult {
    // Go wraps each target in context.WithTimeout(ctx, opts.Timeout), so the
    // deadline reported in a timeout message is this target's own budget
    // rather than the caller's context.
    let deadline = Instant::now() + opts.timeout;
    if let Some(result) = timeout_result(target_name, ctx, deadline) {
        return result;
    }

    let library_path = Path::new(&opts.library_dir);
    let entries = match std::fs::read_dir(library_path) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return TargetResult {
                target: target_name.to_string(),
                status: "ok".to_string(),
                message: Some("no skills rendered".to_string()),
            };
        }
        Err(err) => {
            return TargetResult {
                target: target_name.to_string(),
                status: "error".to_string(),
                message: Some(format!("read skills library: {err}")),
            };
        }
    };

    let entries: Vec<_> = entries.flatten().collect();
    if entries.is_empty() {
        return TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some("no skills rendered".to_string()),
        };
    }

    let mut done = 0;
    let mut failed = Vec::new();

    for entry in entries {
        if let Some(result) = timeout_result(target_name, ctx, deadline) {
            return result;
        }

        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip non-directories and hidden entries
        let is_dir = match entry.file_type() {
            Ok(ft) => ft.is_dir(),
            Err(_) => false,
        };
        if !is_dir || name_str.starts_with('.') {
            continue;
        }

        let skill_root = library_path.join(&*name_str);
        let skill_md_path = skill_root.join("SKILL.md");
        if !skill_md_path.exists() {
            continue; // not a skill directory
        }

        // Load the skill bundle
        let bundle = match load_bundle(&skill_root) {
            Ok(b) => b,
            Err(err) => {
                failed.push(format!("{name_str}: load: {err}"));
                continue;
            }
        };

        // Check if target is explicitly disabled
        let disabled = bundle
            .manifest
            .targets
            .get(target_name)
            .is_some_and(|target_cfg| !target_cfg.enabled);

        if disabled {
            continue; // deliberate opt-out, not a failure
        }

        if dry_run {
            if let Err(err) = plan_skill(&bundle, target_name, opts) {
                failed.push(err.to_string()); // error already includes skill name prefix
                continue;
            }
            done += 1;
            continue;
        }

        if let Err(err) = install_skill(&bundle, target_name, opts) {
            failed.push(err.to_string()); // error already includes skill name prefix
            continue;
        }
        done += 1;
    }

    if let Some(result) = timeout_result(target_name, ctx, deadline) {
        return result;
    }

    if !failed.is_empty() {
        return TargetResult {
            target: target_name.to_string(),
            status: "error".to_string(),
            message: Some(failed.join("; ")),
        };
    }

    if done == 0 {
        return TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some("no skills rendered".to_string()),
        };
    }

    if dry_run {
        return TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some(format!("{done} skills planned")),
        };
    }

    TargetResult {
        target: target_name.to_string(),
        status: "ok".to_string(),
        message: Some(format!("{done} skills rendered and installed")),
    }
}

/// Returns Go's timeout result when this target's deadline has elapsed or the
/// caller's context was cancelled, and None while work may continue.
///
/// Go formats the message as `sync timed out: ` followed by `ctx.Err()` on the
/// per-target context created with `context.WithTimeout`: that child reports
/// "context deadline exceeded" when its own deadline fired first and
/// "context canceled" when the parent was cancelled before it, which the
/// two-branch check below reproduces.
///
/// ponytail: a parent cancellation detected only after the deadline has also
/// elapsed is reported as a deadline, where Go would report the cancellation.
/// Closing that needs a context that timestamps its own cancellation; the
/// distinction is unobservable in the frozen fixture.
fn timeout_result(
    target_name: &str,
    ctx: &impl Context,
    deadline: Instant,
) -> Option<TargetResult> {
    if !ctx.is_cancelled() && Instant::now() < deadline {
        return None;
    }
    let reason = if Instant::now() >= deadline {
        "context deadline exceeded"
    } else {
        "context canceled"
    };
    Some(TargetResult {
        target: target_name.to_string(),
        status: "error".to_string(),
        message: Some(format!("sync timed out: {reason}")),
    })
}

/// Dry-run: renders a skill in memory and resolves its install destination.
///\n\n/// # Errors\n///\n/// Returns an error if rendering fails or the install path cannot be resolved.
fn plan_skill(bundle: &Bundle, target_name: &str, opts: &Options) -> Result<(), SkillError> {
    // Render the skill for the target
    let rendered =
        render::render_target(bundle, target_name, &render::RenderMetadata::default())
            .map_err(|err| SkillError(format!("{}: render: {err}", bundle.manifest.skill.name)))?;

    // Resolve install path (validates the path can be computed)
    install::install_path_for(
        target_name,
        Path::new(&opts.home_dir),
        if opts.project_dir.is_empty() {
            None
        } else {
            Some(Path::new(&opts.project_dir))
        },
        "user",
        &rendered.name,
    )
    .map_err(|err| {
        SkillError(format!(
            "{}: resolve install path: {err}",
            bundle.manifest.skill.name
        ))
    })?;

    Ok(())
}

/// Installs a skill for the target into the harness skill root.
///\n\n/// # Errors\n///\n/// Returns an error if rendering fails or the install fails.
fn install_skill(bundle: &Bundle, target_name: &str, opts: &Options) -> Result<(), SkillError> {
    // Go's installSkill goes through render.RenderAll, and materialize.go wraps
    // every per-target render error unconditionally as `target <t>: <err>`
    // before the runner prepends `<skill>: render: `. The dry-run path calls
    // render.RenderTarget directly and carries no such prefix — both shapes are
    // frozen in the fixture (`failure_visible_and_reported` vs
    // `dry_run_failure_names_target`).
    //
    // ponytail: the Rust path renders in memory and hands the result to
    // install_rendered instead of writing the tree through opts.render_dir the
    // way RenderAll does, so rendered files land in the install cache rather than
    // under the configured render dir. Result messages and error text match Go;
    // the render-dir side effect does not, and closing it needs its own oracle
    // (see migration/implementation-plan.md).
    let rendered = render::render_target(bundle, target_name, &render::RenderMetadata::default())
        .map_err(|err| {
        SkillError(format!(
            "{}: render: target {target_name}: {err}",
            bundle.manifest.skill.name
        ))
    })?;

    let install_opts = install::InstallOptions {
        home_dir: Path::new(&opts.home_dir).to_path_buf(),
        project_dir: if opts.project_dir.is_empty() {
            None
        } else {
            Some(Path::new(&opts.project_dir).to_path_buf())
        },
        base_dir: if opts.base_dir.is_empty() {
            None
        } else {
            Some(Path::new(&opts.base_dir).to_path_buf())
        },
        mode: String::new(), // empty = symlink default (matching Go's empty Mode)
        force: false,
        dry_run: false,
        allow_executable: bundle.manifest.skill.allow_executable,
        fault: None,
        events_path: None,
    };

    install::install_rendered(bundle, &rendered, &install_opts)
        .map_err(|err| SkillError(format!("{}: install: {err}", bundle.manifest.skill.name)))?;

    Ok(())
}

/// Builds the harness -> target map from the canonical harness registry.
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
/// `$HOME/.local/share`; then `base/symbrain/skills`, unless that directory is
/// absent while the legacy `base/symskills` exists, in which case the legacy
/// directory wins.
fn skills_data_root() -> Result<PathBuf, SkillError> {
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

/// Reads `$HOME`, reporting the same error text Go's `os.UserHomeDir` produces.
fn home_dir() -> Result<PathBuf, SkillError> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| SkillError("$HOME is not defined".into()))
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
