//! Native `symbrain skills` CLI implementation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::go_json;
use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{
    self, InstallStatus, MarkerState, StatusKind, StatusOptions, SyncOptions, SyncResult,
    parse_marker,
};
use symbrain_skills::library::list_library;
use symbrain_skills::metadata::{self, Options as MetadataOptions, Record, read_events_log};
use symbrain_skills::parse_skill_md;
use symbrain_skills::targets_status::{
    StatusOptions as TargetStatusOptions, TargetStatus, list_status,
};

/// The shipped `symbrain skills` help, verbatim from the shipped source.
const SKILLS_USAGE: &str = r"symbrain skills — embedded skill library operations

Usage:
  symbrain skills <subcommand> [flags]

Subcommands:
  list        List the skills in the library with their install state
  status      Classify installed skills against the library (drift report)
  targets     Show the harness targets skills can be installed into
  log         Read the local skill operation log
  sync        Repair drifted installs (use --dry-run to see the plan first)
  doctor      Report the configured skill paths and target roots

Use --output table|json (or --json) for the result format. status, sync and
targets accept --scope user|project; status, sync and log accept --target.
Run 'symbrain skills <subcommand> --help' for details.
";

#[derive(Debug, Serialize)]
struct SkillListEntry {
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    category: String,
    path: String,
    #[serde(flatten)]
    record: Record,
}

#[derive(Debug, Serialize)]
struct SkillListReport {
    skills: Vec<SkillListEntry>,
    category_counts: BTreeMap<String, usize>,
    issues: Vec<symbrain_skills::Issue>,
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
struct SkillTargetsReport {
    targets: Vec<TargetStatus>,
}

#[derive(Debug, Serialize)]
struct SkillSyncReport {
    results: Vec<SyncResult>,
    dry_run: bool,
}

/// Resolves the skills data root once for every native skills command.
///
/// Delegates to [`symbrain_skills::runner::skills_data_root`], the resolver
/// whose branches are frozen in the Go runner oracle's `defaults` scenarios:
/// an absolute `$XDG_DATA_HOME` (relative values ignored per the XDG spec),
/// else `$HOME/.local/share`; then `symbrain/skills` unless it is absent
/// while the legacy `symskills` directory exists. Go's `config.Defaults()`
/// resolves through the same `internal/paths.resolve` rule, so this is what
/// keeps native `sync` on the tree Go would read instead of always landing
/// in the current namespace over an empty legacy install (#621).
///
/// The fallback only covers the one branch the runner cannot decide — no
/// absolute `$XDG_DATA_HOME` and no usable home — and preserves the
/// pre-parity resolution for it, because the oracle freezes no case there.
fn skills_data_root() -> PathBuf {
    if let Ok(root) = symbrain_skills::runner::skills_data_root() {
        return root;
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills")
    } else {
        home.join(".local").join("share").join("symskills")
    }
}

pub(crate) fn resolve_skills_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let data_root = skills_data_root();
    let library_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_LIBRARY_DIR") {
        PathBuf::from(path)
    } else {
        data_root.join("library")
    };

    let base_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_BASE_DIR") {
        PathBuf::from(path)
    } else {
        data_root.join("base")
    };

    (library_dir, base_dir, home)
}

fn current_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// Keeps the native status slice limited to the `OpenCode` static contract.
///
/// Custom config, other targets, and every dynamic target state stay on the Go
/// implementation until their byte contract is independently frozen.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    match args.first().map(|arg| arg.to_string_lossy()) {
        Some(verb) if verb == "status" => {
            let Ok((target, scope)) = parse_status_flags(&args[1..]) else {
                return true;
            };
            if has_dynamic_config() || !matches!(scope.as_str(), "user" | "project") {
                return true;
            }
            match target.as_deref() {
                Some("opencode") => opencode_status_needs_go(&scope),
                None => has_dynamic_target_state(),
                Some(_) => true,
            }
        }
        // The native list slice is only the empty-library report, which is the
        // one shape whose bytes are frozen. A populated library needs the Go
        // metadata contract (created/modified times, per-target installs,
        // last-used and the four-column table), so it stays on Go.
        Some(verb) if verb == "list" => {
            parse_list_flags(&args[1..]).is_err() || has_dynamic_config() || library_needs_go()
        }
        // The native targets slice is deliberately only the no-argument,
        // user-scope/default-config contract. Keep every parsed or dynamic
        // variant on Go until its bytes are frozen independently.
        // The native targets inventory observes the harness roots itself, so
        // dynamic target state no longer forces Go. A custom symskills config
        // can still register additional targets, and malformed flags belong to
        // the Go flag package.
        Some(verb) if verb == "targets" => {
            parse_scope_flag(&args[1..]).is_err() || has_dynamic_config()
        }
        Some(verb) if verb == "log" => parse_skill_log_args(&args[1..]).is_err() || has_skill_log(),
        Some(verb) if verb == "doctor" => args.len() != 1 || has_dynamic_config(),
        _ => false,
    }
}

#[derive(Debug, Serialize)]
struct SkillsDoctorVcs {
    enabled: bool,
}

#[derive(Debug, Serialize)]
struct SkillsDoctorConfig {
    library_dir: String,
    render_dir: String,
    cache_dir: String,
    profiles_dir: String,
    base_dir: String,
    #[serde(rename = "Targets")]
    targets: Option<()>,
    vcs: SkillsDoctorVcs,
}

#[derive(Debug, Serialize)]
struct SkillsDoctorTarget {
    target: String,
    user: String,
    project: String,
}

#[derive(Debug, Serialize)]
struct SkillsDoctorReport {
    config: SkillsDoctorConfig,
    config_path: String,
    log_path: String,
    profiles_dir: String,
    project_dir: String,
    targets: Vec<SkillsDoctorTarget>,
}

fn has_skill_log() -> bool {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let current = home.join(".local/share/symskills/events.jsonl");
    log_path_requires_go(&current)
        || log_path_requires_go(&current.with_file_name("events.1.jsonl"))
}

fn log_path_requires_go(path: &std::path::Path) -> bool {
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if metadata.is_symlink() {
                    return true;
                }
                if current == path {
                    if !metadata.is_file() || fs::File::open(path).is_err() {
                        return true;
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if metadata.permissions().mode() & 0o444 == 0 {
                            return true;
                        }
                    }
                } else if !metadata.is_dir() {
                    return true;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // The missing segment itself is fine; existing ancestors still
                // need validation for symlinked or non-directory components.
            }
            Err(_) => return true,
        }
        let Some(parent) = current.parent() else {
            return false;
        };
        if parent == current {
            return false;
        }
        current = parent;
    }
}

#[derive(Debug, Default)]
struct SkillLogArgs {
    skill: Option<String>,
    target: Option<String>,
    limit: i64,
}

fn parse_skill_log_args(args: &[OsString]) -> Result<SkillLogArgs, &'static str> {
    let mut parsed = SkillLogArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        match name {
            "-skill" | "--skill" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| {
                        args.get(index + 1)
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .ok_or("missing skill value")?;
                parsed.skill = Some(value);
                index += usize::from(inline.is_none()) + 1;
            }
            "-target" | "--target" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| {
                        args.get(index + 1)
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .ok_or("missing target value")?;
                parsed.target = Some(value);
                index += usize::from(inline.is_none()) + 1;
            }
            "-limit" | "--limit" | "-l" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| {
                        args.get(index + 1)
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .ok_or("missing limit value")?;
                parsed.limit = value.parse().map_err(|_| "invalid limit")?;
                index += usize::from(inline.is_none()) + 1;
            }
            _ => return Err("unsupported log argument"),
        }
    }
    Ok(parsed)
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
        .or_else(|| symbrain_core::xdg::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_root.join("symskills/config.toml").is_file()
        || current_project_dir().join(".symskills.toml").is_file()
}

/// Reports whether the native `skills list` scan has to stay on Go for this
/// library.
///
/// Go reports an unreadable library and every unloadable skill directory as an
/// `issues[]` entry carrying cap-std error text that is not reproducible here,
/// so any such library is reported by Go instead. A library whose entries all
/// load cleanly — including an absent or empty one — stays native.
fn library_needs_go() -> bool {
    let (library_dir, _, _) = resolve_skills_dirs();
    let entries = match fs::read_dir(&library_dir) {
        Ok(entries) => entries,
        Err(error) => return error.kind() != std::io::ErrorKind::NotFound,
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if !path.is_dir() {
            return false;
        }
        match fs::read(path.join("SKILL.md")) {
            Ok(bytes) => parse_skill_md(&bytes).is_err(),
            Err(_) => true,
        }
    })
}

/// Accepts the flags `skills list` tolerates. Go parses `--target` and
/// `--scope` for this subcommand and then ignores them; every other flag is
/// the flag package's business, so it stays on Go.
fn parse_list_flags(args: &[OsString]) -> Result<(), String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        match name {
            "-target" | "--target" | "-scope" | "--scope" => {
                if inline.is_none() {
                    if args.len() <= index + 1 {
                        return Err(format!("flag needs an argument: {name}"));
                    }
                    index += 1;
                }
                index += 1;
            }
            _ => return Err(format!("flag provided but not defined: {name}")),
        }
    }
    Ok(())
}

fn has_dynamic_target_state() -> bool {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    symbrain_harness::all()
        .iter()
        .filter_map(|harness| harness.skill_target.as_str())
        .any(|target| {
            let skill_root = skill_root_for(target, &home);
            let config_dir = config_dir_for(target, &home);
            fs::symlink_metadata(skill_root).is_ok()
                || fs::metadata(config_dir).is_ok()
                || binary_path_exists(target)
        })
}

/// Keeps the native `status --target opencode` slice inside the byte contract
/// Go actually produces for the current `OpenCode` root of the given scope.
///
/// Go classifies an entry whose marker it cannot fully unmarshal by reusing
/// `encoding/json` error text and by keeping whatever fields it managed to
/// fill; a marker carrying an unknown `schema_version` is accepted with all
/// its fields. Neither is reproducible natively, and the native scan also
/// refuses to follow a second symlink level, so any such entry keeps the whole
/// command on Go instead of emitting different bytes. Roots made of real
/// directories, single-hop links and well-formed schema-version-1 markers stay
/// native.
fn opencode_status_needs_go(scope: &str) -> bool {
    let root = opencode_status_root(scope);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        // A missing root is a normal native case; anything else (for example a
        // permission failure) reports a Go error message we do not mirror.
        Err(error) => return error.kind() != std::io::ErrorKind::NotFound,
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return true;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            return true;
        };
        let marker_dir = if file_type.is_symlink() {
            let resolved = match fs::read_link(&path) {
                Ok(link) if link.is_absolute() => link,
                Ok(link) => path
                    .parent()
                    .map_or_else(|| link.clone(), |parent| parent.join(&link)),
                Err(_) => return true,
            };
            match fs::symlink_metadata(&resolved) {
                // A second link level is not reproducible natively; Go follows it.
                Ok(metadata) if metadata.file_type().is_symlink() => return true,
                // Missing or non-directory targets stay native: both sides
                // report an unmanaged row for them.
                Ok(metadata) if metadata.is_dir() => resolved,
                _ => continue,
            }
        } else if file_type.is_dir() {
            path
        } else {
            continue;
        };
        let marker = marker_dir.join(".symskills.json");
        match fs::read(&marker) {
            Ok(bytes) => {
                if !matches!(parse_marker(&bytes), Ok(MarkerState::Valid(_))) {
                    return true;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
    }
    false
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
    let (entries, issues) = list_library(&library_dir);

    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let log_path = home.join(".local/share/symskills/events.jsonl");
    let metadata_options = MetadataOptions {
        events: read_events_log(&log_path),
        log_path,
        home_dir: home,
        // Go's list scan carries no project directory; user scope ignores it
        // anyway, and the marker fallback must resolve the same roots.
        project_dir: None,
        scope: "user".to_owned(),
    };

    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut skills = Vec::with_capacity(entries.len());
    for entry in entries {
        if !entry.category.is_empty() {
            *category_counts.entry(entry.category.clone()).or_insert(0) += 1;
        }
        let record = metadata::collect(
            std::path::Path::new(&entry.path),
            &entry.name,
            &metadata_options,
        );
        skills.push(SkillListEntry {
            name: entry.name,
            description: entry.description,
            category: entry.category,
            path: entry.path,
            record,
        });
    }

    let report = SkillListReport {
        skills,
        category_counts,
        issues,
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&report));
        }
        OutputFormat::Table => {
            if report.skills.is_empty() {
                let _ = writeln!(stdout, "No skills in the library.");
            } else {
                let _ = writeln!(stdout, "NAME\tCATEGORY\tINSTALLS\tDESCRIPTION");
                for skill in &report.skills {
                    let category = or_dash(&skill.category);
                    let mut targets = skill
                        .record
                        .installs
                        .iter()
                        .map(|install| install.target.as_str())
                        .collect::<Vec<_>>();
                    targets.sort_unstable();
                    let installed = if targets.is_empty() {
                        "-".to_owned()
                    } else {
                        targets.join(",")
                    };
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        skill.name,
                        category,
                        installed,
                        table_content(&skill.description)
                    );
                }
            }
        }
    }

    exit::OK
}

fn or_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

/// Collapses tab, carriage return and newline so one skill stays on one row.
fn table_content(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
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
            // Go encodes every skills report with `json.Encoder`, so the
            // native output needs the same compact shape and HTML escaping.
            let _ = writeln!(stdout, "{}", go_json(&report));
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
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    // Invalid arguments and unknown scope values stay on Go: the flag package
    // and `resolveScope` own those bytes.
    let scope = match parse_scope_flag(args) {
        Ok(scope) => scope,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills targets: {error}");
            return exit::USAGE;
        }
    };
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let options = TargetStatusOptions {
        home_dir: home,
        project_dir: Some(current_project_dir()),
        scope,
    };
    let report = SkillTargetsReport {
        targets: list_status(&options),
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&report));
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT");
            for target in &report.targets {
                let _ = writeln!(
                    stdout,
                    "{}\t{}\t{}\t{}\t{}",
                    target.target,
                    target.installed,
                    target.managed_skills_count,
                    target.unmanaged_skills_count,
                    target.effective_skill_root
                );
            }
        }
    }

    exit::OK
}

/// Parses the `--scope` flag of `skills targets`, accepting `user` and
/// `project` (empty means `user`). Anything else is left to Go.
fn parse_scope_flag(args: &[OsString]) -> Result<String, String> {
    let mut scope = String::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        match name {
            "-scope" | "--scope" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| {
                        args.get(index + 1)
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .ok_or("flag needs an argument: -scope")?;
                value.trim().clone_into(&mut scope);
                index += usize::from(inline.is_none()) + 1;
            }
            _ => return Err(format!("flag provided but not defined: {name}")),
        }
    }
    match scope.as_str() {
        "" | "user" => Ok("user".to_owned()),
        "project" => Ok("project".to_owned()),
        other => Err(format!("unknown scope {other:?} (known: user, project)")),
    }
}

fn skill_root_for(target: &str, home: &std::path::Path) -> PathBuf {
    match target {
        "claude" => native_join(home, ".claude/skills"),
        "opencode" => native_join(home, ".config/opencode/skills"),
        "codex" => native_join(home, ".agents/skills"),
        "antigravity" => native_join(home, ".gemini/config/skills"),
        "hermes" => native_join(home, ".hermes/skills/symaira"),
        "openclaw" => native_join(home, ".openclaw/skills"),
        _ => native_join(home, ".local/share/symskills/skills"),
    }
}

/// Resolves the `OpenCode` skill root the native status slice validates.
///
/// The project root mirrors `symbrain skills doctor` (`<project>/.opencode/skills`)
/// so the fallback decision inspects exactly the directory the native scan will
/// read.
fn opencode_status_root(scope: &str) -> PathBuf {
    if scope == "project" {
        return native_join(&current_project_dir(), ".opencode/skills");
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    skill_root_for("opencode", &home)
}

fn config_dir_for(target: &str, home: &std::path::Path) -> PathBuf {
    match target {
        "claude" => home.join(".claude"),
        "opencode" => native_join(home, ".config/opencode"),
        "codex" => home.join(".codex"),
        "antigravity" => native_join(home, ".gemini/config"),
        "hermes" => home.join(".hermes"),
        "openclaw" => home.join(".openclaw"),
        _ => home.to_path_buf(),
    }
}

fn native_join(base: &std::path::Path, suffix: &str) -> PathBuf {
    #[cfg(windows)]
    {
        suffix
            .split('/')
            .fold(base.to_path_buf(), |path, component| path.join(component))
    }
    #[cfg(not(windows))]
    {
        base.join(suffix)
    }
}

fn binary_path_exists(target: &str) -> bool {
    let name = if target == "antigravity" {
        "agy"
    } else {
        target
    };
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(name);
        candidate.is_file()
            || (cfg!(windows)
                && [".exe", ".cmd", ".bat"]
                    .iter()
                    .any(|suffix| directory.join(format!("{name}{suffix}")).is_file()))
    })
}

fn run_log(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match parse_skill_log_args(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills log: {error}");
            return exit::USAGE;
        }
    };
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = home.join(".local/share/symskills/events.jsonl");
    let skill = parsed
        .skill
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let target = parsed
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut records = match install::read_events(&path, skill, target) {
        Ok(records) => records,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills log: read operation log: {error}");
            return exit::GENERIC;
        }
    };
    records.sort_by(|left, right| right.ts.cmp(&left.ts));
    if parsed.limit > 0 {
        records.truncate(usize::try_from(parsed.limit).unwrap_or(usize::MAX));
    }
    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&records));
        }
        OutputFormat::Table => {
            if records.is_empty() {
                let _ = writeln!(stdout, "No recorded skill operations.");
            } else {
                let _ = writeln!(stdout, "WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME");
                for event in &records {
                    let skill = if event.skill.is_empty() {
                        "-"
                    } else {
                        &event.skill
                    };
                    let target = if event.target.is_empty() {
                        "-"
                    } else {
                        &event.target
                    };
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}\t{}",
                        event.ts, event.event, skill, target, event.outcome
                    );
                }
            }
        }
    }
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
        // Go passes `env.cfg.RenderDir` (the same data root's `rendered/`)
        // into install.Sync, so reinstall writes and links at
        // `<data root>/rendered/<target>/<name>` — legacy root included.
        render_dir: Some(skills_data_root().join("rendered")),
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
            let _ = writeln!(stdout, "{}", go_json(&report));
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
    let (report, pairs) = skills_doctor_report();

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&report));
        }
        OutputFormat::Table => {
            for (name, value) in pairs {
                let _ = writeln!(stdout, "{name:<11} {value}");
            }
        }
    }

    exit::OK
}

fn skills_doctor_report() -> (SkillsDoctorReport, [(&'static str, String); 9]) {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let project_dir = current_project_dir();
    let config_path = skills_config_path();
    let profiles_dir = config_path
        .parent()
        .map_or_else(|| PathBuf::from("profiles"), |path| path.join("profiles"));
    let data_root = symbrain_core::paths::skills_data_dir()
        .map_or_else(|| PathBuf::from("."), |location| location.dir);
    let cache_root = symbrain_core::paths::skills_cache_dir()
        .map_or_else(|| PathBuf::from("."), |location| location.dir);
    let library_dir = data_root.join("library");
    let render_dir = data_root.join("rendered");
    let base_dir = data_root.join("base");
    let log_path = home.join(".local/share/symskills/events.jsonl");
    let config = SkillsDoctorConfig {
        library_dir: library_dir.display().to_string(),
        render_dir: render_dir.display().to_string(),
        cache_dir: cache_root.display().to_string(),
        profiles_dir: profiles_dir.display().to_string(),
        base_dir: base_dir.display().to_string(),
        targets: None,
        vcs: SkillsDoctorVcs { enabled: true },
    };
    let targets = symbrain_skills::default_targets()
        .into_iter()
        .filter_map(|target| {
            let user = skill_root_for(&target, &home);
            let project = doctor_project_skill_root(&target, &project_dir)?;
            Some(SkillsDoctorTarget {
                target,
                user: user.display().to_string(),
                project: project.display().to_string(),
            })
        })
        .collect();
    let report = SkillsDoctorReport {
        config,
        config_path: config_path.display().to_string(),
        log_path: log_path.display().to_string(),
        profiles_dir: profiles_dir.display().to_string(),
        project_dir: project_dir.display().to_string(),
        targets,
    };
    let pairs = [
        ("config", report.config_path.clone()),
        ("library", report.config.library_dir.clone()),
        ("rendered", report.config.render_dir.clone()),
        ("cache", report.config.cache_dir.clone()),
        ("base", report.config.base_dir.clone()),
        ("profiles", report.profiles_dir.clone()),
        ("log", report.log_path.clone()),
        ("project", report.project_dir.clone()),
        ("versioning", report.config.vcs.enabled.to_string()),
    ];
    (report, pairs)
}

fn skills_config_path() -> PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| symbrain_core::xdg::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    root.join("symskills/config.toml")
}

fn doctor_project_skill_root(target: &str, project: &std::path::Path) -> Option<PathBuf> {
    let root = match target {
        "claude" => native_join(project, ".claude/skills"),
        "opencode" => native_join(project, ".opencode/skills"),
        "codex" | "antigravity" | "openclaw" => native_join(project, ".agents/skills"),
        "hermes" => native_join(project, ".hermes/skills"),
        _ => return None,
    };
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::current_project_dir;

    #[test]
    fn sync_project_dir_matches_absolute_working_directory() {
        assert!(current_project_dir().is_absolute());
    }
}
