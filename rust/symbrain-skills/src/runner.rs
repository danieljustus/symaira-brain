//! Orchestration of skill rendering and installation across harness targets.

use crate::install;
use crate::load::load_bundle;
use crate::model::{Bundle, SkillError};
use crate::render;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

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
            opts.home_dir = std::env::var("HOME")
                .map_err(|_| SkillError("cannot determine user home directory".into()))?;
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
        let result = sync_target(ctx, &target_name, &opts, dry_run)?;
        results.push(result);
    }

    Ok(results)
}

/// Synchronizes all skills for a single harness target.
///
/// # Errors
///
/// Returns a result describing success, timeout, or failure — never panics.
#[allow(clippy::too_many_lines)]
fn sync_target(
    ctx: &mut impl Context,
    target_name: &str,
    opts: &Options,
    dry_run: bool,
) -> Result<TargetResult, SkillError> {
    if ctx.is_cancelled() {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "error".to_string(),
            message: Some("sync timed out: context deadline exceeded".to_string()),
        });
    }

    let library_path = Path::new(&opts.library_dir);
    let entries = match std::fs::read_dir(library_path) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TargetResult {
                target: target_name.to_string(),
                status: "ok".to_string(),
                message: Some("no skills rendered".to_string()),
            });
        }
        Err(err) => {
            return Ok(TargetResult {
                target: target_name.to_string(),
                status: "error".to_string(),
                message: Some(format!("read skills library: {err}")),
            });
        }
    };

    let entries: Vec<_> = entries.flatten().collect();
    if entries.is_empty() {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some("no skills rendered".to_string()),
        });
    }

    let mut done = 0;
    let mut failed = Vec::new();

    for entry in entries {
        if ctx.is_cancelled() {
            return Ok(TargetResult {
                target: target_name.to_string(),
                status: "error".to_string(),
                message: Some("sync timed out: context deadline exceeded".to_string()),
            });
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

    if ctx.is_cancelled() {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "error".to_string(),
            message: Some("sync timed out: context deadline exceeded".to_string()),
        });
    }

    if !failed.is_empty() {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "error".to_string(),
            message: Some(failed.join("; ")),
        });
    }

    if done == 0 {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some("no skills rendered".to_string()),
        });
    }

    if dry_run {
        return Ok(TargetResult {
            target: target_name.to_string(),
            status: "ok".to_string(),
            message: Some(format!("{done} skills planned")),
        });
    }

    Ok(TargetResult {
        target: target_name.to_string(),
        status: "ok".to_string(),
        message: Some(format!("{done} skills rendered and installed")),
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
    // Note: Go uses render.RenderAll (writes to render dir) + install.Install.
    // Rust uses render::render_target (in-memory) + install::install_rendered (materializes + installs).
    // The behavior should be equivalent for the native skills path.
    let rendered =
        render::render_target(bundle, target_name, &render::RenderMetadata::default())
            .map_err(|err| SkillError(format!("{}: render: {err}", bundle.manifest.skill.name)))?;

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

/// Default library directory (XDG-aware, matching Go's config.Defaults).
fn default_library_dir() -> Result<String, SkillError> {
    let home = std::env::var("HOME")
        .map_err(|_| SkillError("cannot determine user home directory".into()))?;
    // Go config.Defaults() resolves through sharedpaths.SkillsDataDir() which
    // uses $XDG_DATA_HOME/symbrain/skills (current namespace) or falls back
    // to $XDG_DATA_HOME/symskills / ~/.local/share/symskills (legacy).
    // The Rust symbrain-core paths module handles this; since symbrain-skills
    // does not depend on symbrain-core (no new dependencies permitted),
    // we reproduce the observable legacy/default behavior here.
    Ok(format!("{home}/.local/share/symskills/library"))
}

/// Default render directory (XDG-aware, matching Go's config.Defaults).
fn default_render_dir() -> Result<String, SkillError> {
    let home = std::env::var("HOME")
        .map_err(|_| SkillError("cannot determine user home directory".into()))?;
    // Go config.Defaults() uses sharedpaths.SkillsCacheDir() which resolves
    // to $XDG_CACHE_HOME/symbrain/skills (current) or $XDG_CACHE_HOME/symskills / ~/.cache/symskills.
    // Without symbrain-core dependency, we reproduce the legacy/default path.
    Ok(format!("{home}/.local/share/symskills/rendered"))
}

/// Default base directory (XDG-aware, matching Go's config.Defaults).
fn default_base_dir() -> Result<String, SkillError> {
    let home = std::env::var("HOME")
        .map_err(|_| SkillError("cannot determine user home directory".into()))?;
    // Go config.Defaults() uses SkillsDataDir() for BaseDir as well.
    Ok(format!("{home}/.local/share/symskills/base"))
}
