//! Native `symbrain sync` CLI implementation.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use symbrain_adapter::{target_for_harness, write_atomic};
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_harness::{all, lookup};
use symbrain_instructions::Source;

/// Outcome of syncing one instruction target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetStatus {
    /// Harness name.
    pub name: String,
    /// Absolute or relative path to the synced instruction file.
    pub path: String,
    /// Status: created, updated, unchanged, dry-run, skipped, error.
    pub status: String,
    /// Optional diagnostic or explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Outcome of syncing skills for one harness target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillResult {
    /// Harness target name.
    pub target: String,
    /// Status: ok, skipped, error.
    pub status: String,
    /// Optional summary message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// JSON payload returned by `symbrain sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncSummary {
    /// Instruction target statuses.
    pub targets: Vec<TargetStatus>,
    /// Skills sync results per harness.
    pub skills: Vec<SkillResult>,
}

struct ParsedSyncArgs {
    project_dir: PathBuf,
    dry_run: bool,
    harnesses: Vec<String>,
}

/// Returns whether `sync` still belongs to the Go oracle.
///
/// Native sync currently covers only the explicit `agents` instruction target
/// with the default project directory. Skill rendering/install, project
/// overrides, and implicit all-harness selection remain Go-owned until their
/// side effects are covered by a differential fixture.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    let mut saw_harness = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        match arg.as_ref() {
            "-dry-run" | "--dry-run" => i += 1,
            "-project" | "--project" => return true,
            value if value.starts_with("-project=") || value.starts_with("--project=") => {
                return true;
            }
            "--" => return true,
            value if value.starts_with('-') => return true,
            value => {
                if value != "agents" {
                    return true;
                }
                saw_harness = true;
                i += 1;
            }
        }
    }
    !saw_harness
}

fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<ParsedSyncArgs, u8> {
    let mut project_dir = PathBuf::from(".");
    let mut dry_run = false;
    let mut harnesses = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-dry-run" || arg == "--dry-run" {
            dry_run = true;
            i += 1;
        } else if arg == "-project" || arg == "--project" {
            if i + 1 >= args.len() {
                let _ = writeln!(stderr, "symbrain sync: flag needs an argument: {arg}");
                return Err(exit::USAGE);
            }
            project_dir = PathBuf::from(&args[i + 1]);
            i += 2;
        } else if let Some(val) = arg
            .strip_prefix("-project=")
            .or_else(|| arg.strip_prefix("--project="))
        {
            project_dir = PathBuf::from(val);
            i += 1;
        } else if arg.starts_with('-') {
            let _ = writeln!(stderr, "symbrain sync: unknown flag: {arg}");
            return Err(exit::USAGE);
        } else {
            harnesses.push(arg.into_owned());
            i += 1;
        }
    }

    Ok(ParsedSyncArgs {
        project_dir,
        dry_run,
        harnesses,
    })
}

/// Executes `symbrain sync`.
#[allow(clippy::too_many_lines)]
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match parse_args(args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };

    // Validate harness names
    let requested_harnesses = if parsed.harnesses.is_empty() {
        all()
            .iter()
            .map(|h| h.name.as_str().to_string())
            .collect::<Vec<_>>()
    } else {
        for name in &parsed.harnesses {
            if lookup(name).is_err() {
                let _ = writeln!(
                    stderr,
                    "symbrain sync: unknown harness {name:?}; want one of: {}",
                    symbrain_harness::names().join(", ")
                );
                return exit::USAGE;
            }
        }
        parsed.harnesses.clone()
    };

    // Load instruction source
    let source = Source::new(Some(&parsed.project_dir));
    let instruction_content = match source.content() {
        Ok(content) => content,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain sync: load instructions: {err}");
            return exit::GENERIC;
        }
    };

    let mut target_statuses = Vec::new();
    for name in &requested_harnesses {
        let harness_name = match lookup(name) {
            Ok(h) => h.name,
            Err(_) => continue,
        };

        let target = match target_for_harness(harness_name.as_str()) {
            Ok(Some(t)) => t,
            Ok(None) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: String::new(),
                    status: "skipped".to_string(),
                    message: Some(format!("harness {name:?} has no instruction adapter")),
                });
                continue;
            }
            Err(err) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: String::new(),
                    status: "error".to_string(),
                    message: Some(format!("resolve target: {err}")),
                });
                continue;
            }
        };

        let target_path = match target.path(&parsed.project_dir) {
            Ok(p) => p,
            Err(err) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: String::new(),
                    status: "error".to_string(),
                    message: Some(err.to_string()),
                });
                continue;
            }
        };

        let existing_bytes = fs::read(&target_path).unwrap_or_default();
        let rendered =
            match target.render(&existing_bytes, &instruction_content, &parsed.project_dir) {
                Ok(r) => r,
                Err(err) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: target_path.to_string_lossy().into_owned(),
                        status: "error".to_string(),
                        message: Some(err.to_string()),
                    });
                    continue;
                }
            };

        if rendered.output == existing_bytes {
            target_statuses.push(TargetStatus {
                name: name.clone(),
                path: target_path.to_string_lossy().into_owned(),
                status: "unchanged".to_string(),
                message: None,
            });
        } else if parsed.dry_run {
            target_statuses.push(TargetStatus {
                name: name.clone(),
                path: target_path.to_string_lossy().into_owned(),
                status: "dry-run".to_string(),
                message: Some("would update".to_string()),
            });
        } else {
            let existed = target_path.exists();
            match write_atomic(
                &parsed.project_dir,
                Path::new(target.relative_path()),
                &rendered.output,
            ) {
                Ok(()) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: target_path.to_string_lossy().into_owned(),
                        status: if existed { "updated" } else { "created" }.to_string(),
                        message: None,
                    });
                }
                Err(err) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: target_path.to_string_lossy().into_owned(),
                        status: "error".to_string(),
                        message: Some(err.to_string()),
                    });
                }
            }
        }
    }

    let summary = SyncSummary {
        targets: target_statuses,
        skills: requested_harnesses
            .iter()
            .map(|name| SkillResult {
                target: name.clone(),
                status: "skipped".to_string(),
                message: Some(format!("no skill target for harness {name:?}")),
            })
            .collect(),
    };

    let has_error = summary.targets.iter().any(|t| t.status == "error")
        || summary.skills.iter().any(|s| s.status == "error");

    match format {
        OutputFormat::Json => {
            let _ = symbrain_core::output::render_json(&mut *stdout, &summary);
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "Instruction targets:");
            for t in &summary.targets {
                let msg = t
                    .message
                    .as_deref()
                    .map(|m| format!(" ({m})"))
                    .unwrap_or_default();
                let _ = writeln!(
                    stdout,
                    "  {:<12} {}{}",
                    format!("{}:", t.name),
                    t.status,
                    msg
                );
            }
            if !summary.skills.is_empty() {
                let _ = writeln!(stdout, "\nSkills:");
                for s in &summary.skills {
                    let msg = s
                        .message
                        .as_deref()
                        .map(|m| format!(" ({m})"))
                        .unwrap_or_default();
                    let _ = writeln!(
                        stdout,
                        "  {:<12} {}{}",
                        format!("{}:", s.target),
                        s.status,
                        msg
                    );
                }
            }
        }
    }

    if has_error { exit::GENERIC } else { exit::OK }
}
