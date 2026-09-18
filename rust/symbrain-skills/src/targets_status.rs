//! Read-only harness target inventory behind `symbrain skills targets`.
//!
//! Every field is observed, never guessed: evidence names the file or binary
//! it came from, and a missing skill root is reported as `missing` rather than
//! as an empty installation. Declared runtime capabilities stay `unknown`
//! unless the target registry states them, because symskills does not inspect
//! harness runtimes.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::target::{self, TargetSpec, lookup};

/// What symskills can do toward a target, in report order.
const CAPABILITIES: [&str; 4] = ["render", "install", "symlink", "copy"];

/// Runtime capability vocabulary, in report order. Each entry is reported for
/// every target with the state the registry declares for it.
const RUNTIME_CAPABILITIES: [&str; 6] = [
    "subagents",
    "background_tasks",
    "mcp",
    "slash_commands",
    "hooks",
    "scheduled_tasks",
];

const CAPABILITY_SUPPORTED: &str = "supported";
const CAPABILITY_UNSUPPORTED: &str = "unsupported";
const CAPABILITY_UNKNOWN: &str = "unknown";

const MARKER_FILE: &str = ".symskills.json";

/// One row of `skills targets`.
///
/// Field order matches the Go struct so the JSON bytes match too.
#[derive(Debug, Serialize)]
pub struct TargetStatus {
    /// Stable target name.
    pub target: String,
    /// Human-readable target name.
    pub display_name: String,
    /// Whether any harness evidence (binary or config directory) was found.
    pub installed: bool,
    /// First evidence source(s), e.g. `binary:/usr/local/bin/opencode`.
    pub evidence: String,
    /// Skill root the target would read for the requested scope.
    pub effective_skill_root: String,
    /// Whether the skill root exists on disk.
    pub skill_root_exists: bool,
    /// Whether the skill root could be enumerated.
    pub skill_root_readable: bool,
    /// Installs carrying a symskills marker.
    pub managed_skills_count: usize,
    /// Unmanaged directories or links in the skill root.
    pub unmanaged_skills_count: usize,
    /// `missing` | `unreadable` | `managed` | `mixed` | `unmanaged` | `empty`.
    pub install_state: String,
    /// What symskills can do toward this target.
    pub capabilities: Vec<&'static str>,
    /// Declared runtime capability states.
    pub runtime_capabilities: BTreeMap<String, String>,
    /// Ready-to-run next step for this target.
    pub setup_hint: String,
    /// `verified` when evidence was found, otherwise `not_verified`.
    pub verification_status: String,
}

/// Inputs for a target inventory scan.
#[derive(Debug, Clone, Default)]
pub struct StatusOptions {
    /// User home used to resolve harness roots.
    pub home_dir: PathBuf,
    /// Project root; empty disables project-scope resolution.
    pub project_dir: Option<PathBuf>,
    /// `user` (default) or `project`.
    pub scope: String,
}

/// Returns one status row per registered target, in registry order.
#[must_use]
pub fn list_status(options: &StatusOptions) -> Vec<TargetStatus> {
    target::target_names()
        .iter()
        .filter_map(|name| lookup(name))
        .map(|spec| inspect(spec, options))
        .collect()
}

fn inspect(spec: TargetSpec, options: &StatusOptions) -> TargetStatus {
    let scope = if options.scope.is_empty() {
        "user"
    } else {
        options.scope.as_str()
    };
    let project = options.project_dir.as_deref();
    let skill_root = target::skill_root(spec.name, &options.home_dir, project, scope)
        .unwrap_or_else(|| options.home_dir.clone());
    let config_dir = target::config_dir(spec.name, &options.home_dir, project, scope);

    let evidence = evidence_for(spec, config_dir.as_deref());
    let counts = SkillRootCounts::scan(&skill_root);
    let (install_state, setup_hint) = state_and_hint(spec, &skill_root, &counts);
    let runtime_capabilities = RUNTIME_CAPABILITIES
        .iter()
        .map(|name| ((*name).to_owned(), capability_state(spec, name).to_owned()))
        .collect();

    TargetStatus {
        target: spec.name.to_owned(),
        display_name: spec.display_name.to_owned(),
        installed: evidence.installed,
        evidence: evidence.text,
        effective_skill_root: skill_root.display().to_string(),
        skill_root_exists: counts.exists,
        skill_root_readable: counts.readable,
        managed_skills_count: counts.managed,
        unmanaged_skills_count: counts.unmanaged,
        install_state,
        capabilities: CAPABILITIES.to_vec(),
        runtime_capabilities,
        setup_hint,
        verification_status: evidence.verification_status,
    }
}

/// Harness evidence: an installed binary and/or a present config directory.
struct Evidence {
    installed: bool,
    text: String,
    verification_status: String,
}

fn evidence_for(spec: TargetSpec, config_dir: Option<&Path>) -> Evidence {
    let mut parts: Vec<String> = Vec::new();
    if let Some(binary) = lookup_path(spec.binary_name) {
        parts.push(format!("binary:{}", binary.display()));
    }
    if let Some(config) = config_dir
        && config.is_dir()
    {
        parts.push(format!("config_dir:{}", config.display()));
    }
    let installed = !parts.is_empty();
    let text = match parts.len() {
        0 => "none".to_owned(),
        1 => parts[0].clone(),
        _ => format!("{},{}", parts[0], parts[1]),
    };
    Evidence {
        installed,
        text,
        verification_status: if installed {
            "verified"
        } else {
            "not_verified"
        }
        .to_owned(),
    }
}

/// Observed skill-root state for one target.
struct SkillRootCounts {
    exists: bool,
    readable: bool,
    managed: usize,
    unmanaged: usize,
}

impl SkillRootCounts {
    fn scan(skill_root: &Path) -> Self {
        let mut counts = Self {
            exists: false,
            readable: false,
            managed: 0,
            unmanaged: 0,
        };
        if fs::symlink_metadata(skill_root).is_err() {
            return counts;
        }
        counts.exists = true;
        let Ok(entries) = fs::read_dir(skill_root) else {
            return counts;
        };
        counts.readable = true;
        for entry in entries.flatten() {
            let path = skill_root.join(entry.file_name());
            if is_managed_skill(&path) {
                counts.managed += 1;
            } else if entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() || kind.is_symlink())
            {
                counts.unmanaged += 1;
            }
        }
        counts
    }
}

/// Derives the reported install state and the ready-to-run setup hint.
fn state_and_hint(
    spec: TargetSpec,
    skill_root: &Path,
    counts: &SkillRootCounts,
) -> (String, String) {
    let managed = counts.managed;
    let unmanaged = counts.unmanaged;
    if !counts.exists {
        return (
            "missing".to_owned(),
            format!(
                "Create skill directory {} or run 'symskills install --target {} <skill>'",
                skill_root.display(),
                spec.name
            ),
        );
    }
    if !counts.readable {
        return (
            "unreadable".to_owned(),
            format!("Check permissions for skill root {}", skill_root.display()),
        );
    }
    if managed > 0 && unmanaged == 0 {
        return (
            "managed".to_owned(),
            format!("Harness is active with {managed} managed skill(s)"),
        );
    }
    if managed > 0 {
        return (
            "mixed".to_owned(),
            format!("Harness contains {managed} managed and {unmanaged} unmanaged skill(s)"),
        );
    }
    if unmanaged > 0 {
        return (
            "unmanaged".to_owned(),
            format!(
                "Harness contains {unmanaged} unmanaged skill(s); consider importing into library"
            ),
        );
    }
    (
        "empty".to_owned(),
        format!(
            "Harness skill directory is ready; install skills with 'symskills install --target {} <skill>'",
            spec.name
        ),
    )
}

/// Declared state of one runtime capability for a target.
fn capability_state(spec: TargetSpec, name: &str) -> &'static str {
    match spec
        .capabilities
        .iter()
        .find(|(capability, _)| *capability == name)
    {
        None => CAPABILITY_UNKNOWN,
        Some((_, true)) => CAPABILITY_SUPPORTED,
        Some((_, false)) => CAPABILITY_UNSUPPORTED,
    }
}

/// Reports whether an entry in a skill root is a symskills-managed install.
///
/// A marker directly inside the entry counts, and so does a marker behind a
/// link, because that is where symlink-mode installs keep it.
fn is_managed_skill(path: &Path) -> bool {
    if fs::metadata(path.join(MARKER_FILE)).is_ok() {
        return true;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(link) = fs::read_link(path) else {
        return false;
    };
    let resolved = if link.is_absolute() {
        link
    } else {
        path.parent()
            .map_or(link.clone(), |parent| parent.join(&link))
    };
    fs::metadata(resolved.join(MARKER_FILE)).is_ok()
}

/// Resolves an executable name on `PATH` the way the Go implementation does,
/// including the empty `PATH` entry (current directory) and `PATHEXT`
/// suffixes on Windows.
fn lookup_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let suffixes: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for directory in env::split_paths(&path) {
        for suffix in suffixes {
            let candidate = if suffix.is_empty() {
                name.to_owned()
            } else {
                format!("{name}{suffix}")
            };
            let candidate = if directory.as_os_str().is_empty() {
                PathBuf::from(candidate)
            } else {
                directory.join(candidate)
            };
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        drop(metadata);
        true
    }
}
