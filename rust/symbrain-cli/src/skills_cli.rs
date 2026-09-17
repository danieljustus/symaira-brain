//! Native `symbrain skills` CLI implementation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{
    self, InstallStatus, StatusKind, StatusOptions, SyncOptions, SyncResult,
};
use symbrain_skills::load_bundle;

const SKILLS_USAGE: &str = "symbrain skills — operate the embedded skill library

Usage:
  symbrain skills list
  symbrain skills status [--target NAME] [--scope user|project]
  symbrain skills targets [--scope user|project]
  symbrain skills log [--skill NAME] [--target NAME] [--limit N]
  symbrain skills sync [--target NAME] [--scope user|project] [--dry-run]
  symbrain skills doctor

The global --output table|json flag (or --json) selects the output format.
";

#[derive(Debug, Serialize)]
struct SkillListEntry {
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    category: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct SkillListReport {
    skills: Vec<SkillListEntry>,
    category_counts: BTreeMap<String, usize>,
    issues: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct SkillStatusSummary {
    in_sync: usize,
    stale: usize,
    harness_changed: usize,
    conflict: usize,
    orphaned: usize,
    unmanaged: usize,
}

#[derive(Debug, Serialize)]
struct SkillStatusReport {
    installs: Vec<InstallStatus>,
    summary: SkillStatusSummary,
}

#[derive(Debug, Serialize)]
struct SkillTargetsEntry {
    target: String,
    installed: bool,
    managed_count: usize,
    skill_root: String,
}

#[derive(Debug, Serialize)]
struct SkillTargetsReport {
    targets: Vec<SkillTargetsEntry>,
}

#[derive(Debug, Serialize)]
struct SkillSyncReport {
    results: Vec<SyncResult>,
    dry_run: bool,
}

fn resolve_skills_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let library_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_LIBRARY_DIR") {
        PathBuf::from(path)
    } else if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills").join("library")
    } else {
        home.join(".local")
            .join("share")
            .join("symskills")
            .join("library")
    };

    let base_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_BASE_DIR") {
        PathBuf::from(path)
    } else if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills").join("base")
    } else {
        home.join(".local")
            .join("share")
            .join("symskills")
            .join("base")
    };

    (library_dir, base_dir, home)
}

fn current_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// Keeps the native slice limited to the frozen `OpenCode` user-scope contract.
///
/// Other targets, project scope, custom config, and dynamic target cases stay
/// on the Go implementation until their byte contract is independently frozen.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    if args.first().map(|arg| arg.to_string_lossy()) != Some("status".into()) {
        return false;
    }
    let Ok((target, scope)) = parse_status_flags(&args[1..]) else {
        return true;
    };
    target.as_deref() != Some("opencode") || scope != "user" || has_dynamic_config()
}

fn has_dynamic_config() -> bool {
    const CONFIG_OVERRIDES: &[&str] = &[
        "SYMSKILLS_LIBRARY_DIR",
        "SYMSKILLS_RENDER_DIR",
        "SYMSKILLS_CACHE_DIR",
        "SYMSKILLS_PROFILES_DIR",
        "SYMSKILLS_BASE_DIR",
        "SYMSKILLS_VCS_ENABLED",
        "SYMBRAIN_SKILLS_LIBRARY_DIR",
        "SYMBRAIN_SKILLS_BASE_DIR",
    ];
    if CONFIG_OVERRIDES
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return true;
    }
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| symbrain_core::xdg::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_root.join("symskills/config.toml").is_file()
        || current_project_dir().join(".symskills.toml").is_file()
}

fn parse_status_flags(args: &[OsString]) -> Result<(Option<String>, String), &'static str> {
    let mut target = None;
    let mut scope = "user".to_owned();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-target" || arg == "--target" {
            let Some(value) = args.get(i + 1) else {
                return Err("missing target value");
            };
            target = Some(value.to_string_lossy().into_owned());
            i += 2;
        } else if let Some(value) = arg
            .strip_prefix("-target=")
            .or_else(|| arg.strip_prefix("--target="))
        {
            target = Some(value.to_owned());
            i += 1;
        } else if arg == "-scope" || arg == "--scope" {
            let Some(value) = args.get(i + 1) else {
                return Err("missing scope value");
            };
            scope = value.to_string_lossy().into_owned();
            i += 2;
        } else if let Some(value) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
        {
            value.clone_into(&mut scope);
            i += 1;
        } else {
            return Err("unsupported status argument");
        }
    }
    Ok((target, scope))
}

fn status_name(status: StatusKind) -> &'static str {
    match status {
        StatusKind::InSync => "in-sync",
        StatusKind::Stale => "stale",
        StatusKind::HarnessChanged => "harness-changed",
        StatusKind::Conflict => "conflict",
        StatusKind::Converged => "converged",
        StatusKind::Orphaned => "orphaned",
        StatusKind::Unmanaged => "unmanaged",
    }
}

/// Runs `symbrain skills`.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if args.is_empty() {
        let _ = write!(stderr, "{SKILLS_USAGE}");
        return exit::USAGE;
    }

    let verb = args[0].to_string_lossy();
    let rest = &args[1..];

    match verb.as_ref() {
        "-h" | "--help" | "help" => {
            let _ = write!(stdout, "{SKILLS_USAGE}");
            exit::OK
        }
        "list" => run_list(rest, stdout, stderr, format),
        "status" => run_status(rest, stdout, stderr, format),
        "targets" => run_targets(rest, stdout, stderr, format),
        "log" => run_log(rest, stdout, stderr, format),
        "sync" => run_sync(rest, stdout, stderr, format),
        "doctor" => run_doctor(rest, stdout, stderr, format),
        _ => {
            let _ = writeln!(stderr, "symbrain skills: unknown subcommand {verb:?}\n");
            let _ = write!(stderr, "{SKILLS_USAGE}");
            exit::USAGE
        }
    }
}

fn run_list(
    _args: &[OsString],
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let (library_dir, _, _) = resolve_skills_dirs();
    let mut entries = Vec::new();
    let mut category_counts = BTreeMap::new();
    let mut issues = Vec::new();

    if let Ok(dir_entries) = fs::read_dir(&library_dir) {
        let mut paths: Vec<_> = dir_entries.flatten().map(|e| e.path()).collect();
        paths.sort();

        for path in paths {
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with('.'))
            {
                continue;
            }
            if !path.join("SKILL.md").exists() {
                continue;
            }
            match load_bundle(&path) {
                Ok(bundle) => {
                    let cat = bundle.frontmatter.category.clone();
                    if !cat.is_empty() {
                        *category_counts.entry(cat.clone()).or_insert(0) += 1;
                    }
                    entries.push(SkillListEntry {
                        name: bundle.frontmatter.name.clone(),
                        description: bundle.frontmatter.description.clone(),
                        category: cat,
                        path: path.to_string_lossy().into_owned(),
                    });
                }
                Err(err) => {
                    issues.push(format!("{}: {err}", path.display()));
                }
            }
        }
    }

    let report = SkillListReport {
        skills: entries,
        category_counts,
        issues,
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if report.skills.is_empty() {
                let _ = writeln!(stdout, "No skills in the library.");
            } else {
                let _ = writeln!(stdout, "NAME\tCATEGORY\tDESCRIPTION");
                for s in &report.skills {
                    let cat = if s.category.is_empty() {
                        "-"
                    } else {
                        &s.category
                    };
                    let _ = writeln!(stdout, "{}\t{}\t{}", s.name, cat, s.description);
                }
            }
        }
    }

    exit::OK
}

fn run_status(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let (target, scope) = match parse_status_flags(args) {
        Ok(values) => values,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills status: {error}");
            return exit::USAGE;
        }
    };
    if let Some(target_name) = target.as_deref()
        && !symbrain_skills::default_targets()
            .iter()
            .any(|known| known == target_name)
    {
        let _ = writeln!(
            stderr,
            "symbrain skills status: unknown target {target_name:?}"
        );
        return exit::USAGE;
    }
    if scope != "user" && scope != "project" {
        let _ = writeln!(stderr, "symbrain skills status: unknown scope {scope:?}");
        return exit::USAGE;
    }

    let (library_dir, base_dir, home_dir) = resolve_skills_dirs();
    let project_dir = current_project_dir();
    let targets = target.into_iter().collect();

    let opts = StatusOptions {
        home_dir,
        project_dir: Some(project_dir),
        scope,
        targets,
        library_dir,
        base_dir: Some(base_dir),
        skills: Vec::new(),
    };

    let statuses = match install::status(&opts) {
        Ok(s) => s,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain skills status: scan installs: {err}");
            return exit::GENERIC;
        }
    };

    let mut summary = SkillStatusSummary::default();
    for st in &statuses {
        match st.status {
            StatusKind::InSync | StatusKind::Converged => summary.in_sync += 1,
            StatusKind::Stale => summary.stale += 1,
            StatusKind::HarnessChanged => summary.harness_changed += 1,
            StatusKind::Conflict => summary.conflict += 1,
            StatusKind::Orphaned => summary.orphaned += 1,
            StatusKind::Unmanaged => summary.unmanaged += 1,
        }
    }

    let report = SkillStatusReport {
        installs: statuses,
        summary,
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string(&report).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if report.installs.is_empty() {
                let _ = writeln!(stdout, "No installed skills found.");
                return exit::OK;
            }
            let _ = writeln!(stdout, "TARGET\tSKILL\tSTATUS\tMODE\tPATH");
            for st in &report.installs {
                let mode = st.mode.as_deref().unwrap_or("-");
                let _ = writeln!(
                    stdout,
                    "{}\t{}\t{}\t{}\t{}",
                    st.target,
                    st.name,
                    status_name(st.status),
                    mode,
                    st.path.display()
                );
            }
        }
    }

    exit::OK
}

fn run_targets(
    _args: &[OsString],
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut targets = Vec::new();
    for h in symbrain_harness::all() {
        let Some(skill_target) = h.skill_target.as_str() else {
            continue;
        };
        targets.push(SkillTargetsEntry {
            target: h.name.as_str().to_string(),
            installed: true,
            managed_count: 0,
            skill_root: skill_target.to_string(),
        });
    }

    let report = SkillTargetsReport { targets };
    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "TARGET\tINSTALLED\tMANAGED\tSKILL ROOT");
            for t in &report.targets {
                let _ = writeln!(
                    stdout,
                    "{}\t{}\t{}\t{}",
                    t.target, t.installed, t.managed_count, t.skill_root
                );
            }
        }
    }

    exit::OK
}

fn run_log(
    _args: &[OsString],
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let empty_list: Vec<String> = Vec::new();
    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "[]");
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "No recorded skill operations.");
        }
    }
    let _ = empty_list;
    exit::OK
}

fn run_sync(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut dry_run = false;
    let mut target = None;
    let mut scope = "user".to_string();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-dry-run" || arg == "--dry-run" {
            dry_run = true;
            i += 1;
        } else if arg == "-target" || arg == "--target" {
            if i + 1 < args.len() {
                target = Some(args[i + 1].to_string_lossy().into_owned());
                i += 2;
            }
        } else if let Some(v) = arg
            .strip_prefix("-target=")
            .or_else(|| arg.strip_prefix("--target="))
        {
            target = Some(v.to_string());
            i += 1;
        } else if arg == "-scope" || arg == "--scope" {
            if i + 1 < args.len() {
                scope = args[i + 1].to_string_lossy().into_owned();
                i += 2;
            }
        } else if let Some(v) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
        {
            scope = v.to_string();
            i += 1;
        } else {
            i += 1;
        }
    }

    let (library_dir, base_dir, home_dir) = resolve_skills_dirs();
    let targets = target.into_iter().collect();

    let sync_opts = SyncOptions {
        library_dir,
        home_dir,
        project_dir: Some(current_project_dir()),
        scope,
        targets,
        skills: Vec::new(),
        base_dir: Some(base_dir),
        // An empty mode preserves the marker's original copy/symlink mode.
        mode: String::new(),
        force: false,
        dry_run,
        conflict_policy: install::ConflictPolicy::Abort,
        events_path: None,
    };

    let results = match install::sync(&sync_opts) {
        Ok(r) => r,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain skills sync: {err}");
            return exit::GENERIC;
        }
    };

    let report = SkillSyncReport { results, dry_run };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if report.results.is_empty() {
                let _ = writeln!(stdout, "Every installed skill is in sync.");
            } else {
                let _ = writeln!(stdout, "TARGET\tSKILL\tACTION\tDETAIL");
                for r in &report.results {
                    let detail = if r.error.is_empty() { "-" } else { &r.error };
                    let _ = writeln!(stdout, "{}\t{}\t{}\t{}", r.target, r.name, r.action, detail);
                }
            }
        }
    }

    exit::OK
}

fn run_doctor(
    _args: &[OsString],
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let (library_dir, base_dir, _) = resolve_skills_dirs();
    let doc_info = serde_json::json!({
        "library_dir": library_dir.display().to_string(),
        "base_dir": base_dir.display().to_string(),
        "library_exists": library_dir.exists(),
        "base_exists": base_dir.exists(),
    });

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&doc_info).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            let _ = writeln!(
                stdout,
                "Skills Library: {} (exists: {})",
                library_dir.display(),
                library_dir.exists()
            );
            let _ = writeln!(
                stdout,
                "Base Snapshots: {} (exists: {})",
                base_dir.display(),
                base_dir.exists()
            );
        }
    }

    exit::OK
}

#[cfg(test)]
mod tests {
    use super::current_project_dir;

    #[test]
    fn sync_project_dir_matches_absolute_working_directory() {
        assert!(current_project_dir().is_absolute());
    }
}
