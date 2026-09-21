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

/// The shipped `sync` flag set as the Go flag package prints it.
const SYNC_FLAGS_USAGE: &str = "Usage of sync:\n  -dry-run\n    \tshow what would be written without making changes\n  -project string\n    \tproject directory (default: current directory)\n";

/// Returns whether `sync` still belongs to the Go oracle.
///
/// Native sync covers the explicit `agents` instruction target and the
/// implicit all-harness default, plus its own flag-package-style rejection
/// of unknown flags. Project overrides remain Go-owned (no fixture yet).
/// Skill rendering/install is only proven for the empty-library case
/// (`skillsrunner.Run` reports "no skills rendered" for any missing or
/// empty library without touching the render/install pipeline); a non-empty
/// library still defers to Go because that pipeline is not ported.
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
            // An unrecognized flag is reported natively as a usage error
            // below, matching the Go flag package's own rejection.
            value if value.starts_with('-') => i += 1,
            value => {
                if value != "agents" {
                    return true;
                }
                saw_harness = true;
                i += 1;
            }
        }
    }
    !saw_harness && skills_library_has_entries()
}

/// Whether the resolved skills library directory exists and has any entry.
///
/// A missing or empty library is the only case proven against Go
/// (`skillsrunner.Run` returns "no skills rendered" for it without invoking
/// the render/install pipeline); any entry at all falls back to Go instead
/// of guessing what an unported render pass would report.
fn skills_library_has_entries() -> bool {
    let (library_dir, _base_dir, _home_dir) = crate::skills_cli::resolve_skills_dirs();
    fs::read_dir(library_dir).is_ok_and(|mut entries| entries.next().is_some())
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
        } else if arg == "-h" || arg == "--h" || arg == "-help" || arg == "--help" {
            // Go's flag package special-cases an unregistered "-h"/"-help":
            // it prints the flag set's usage and returns flag.ErrHelp, never
            // "flag provided but not defined" (that message is reserved for
            // every other unrecognized flag).
            let _ = write!(stderr, "{SYNC_FLAGS_USAGE}");
            return Err(exit::USAGE);
        } else if arg.starts_with('-') {
            let trimmed = arg.trim_start_matches('-');
            let name = trimmed.split_once('=').map_or(trimmed, |(name, _)| name);
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = write!(stderr, "{SYNC_FLAGS_USAGE}");
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
        // Go's filepath.Join(".", target) removes the leading "./".
        let display_path = target_path
            .strip_prefix(".")
            .unwrap_or(&target_path)
            .to_string_lossy()
            .into_owned();

        let existing_bytes = fs::read(&target_path).unwrap_or_default();
        let rendered =
            match target.render(&existing_bytes, &instruction_content, &parsed.project_dir) {
                Ok(r) => r,
                Err(err) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: display_path.clone(),
                        status: "error".to_string(),
                        message: Some(err.to_string()),
                    });
                    continue;
                }
            };

        if rendered.output == existing_bytes {
            target_statuses.push(TargetStatus {
                name: name.clone(),
                path: display_path.clone(),
                status: "unchanged".to_string(),
                message: None,
            });
        } else if parsed.dry_run {
            target_statuses.push(TargetStatus {
                name: name.clone(),
                path: display_path.clone(),
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
                        path: display_path.clone(),
                        status: if existed { "updated" } else { "created" }.to_string(),
                        message: None,
                    });
                }
                Err(err) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: display_path,
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
            .map(|name| {
                let has_skill_target =
                    lookup(name).is_ok_and(|h| h.skill_target.as_str().is_some());
                if has_skill_target {
                    // Reachable only when the skills library is empty or
                    // missing (see requires_go_fallback): the shipped
                    // skillsrunner reports exactly this for that case
                    // without touching the render/install pipeline.
                    SkillResult {
                        target: name.clone(),
                        status: "ok".to_string(),
                        message: Some("no skills rendered".to_string()),
                    }
                } else {
                    SkillResult {
                        target: name.clone(),
                        status: "skipped".to_string(),
                        message: Some(format!("no skill target for harness {name:?}")),
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Matches the frozen `go sync --unknown` fixture case byte-for-byte
    /// (see rust/symbrain-cli/tests/fixtures/cli_tree_expectations.json):
    /// exit 2, and the Go flag package's own rejection text.
    #[test]
    fn unknown_flag_matches_go_flag_package_rejection() {
        let mut stderr = Vec::new();
        let result = parse_args(&[OsString::from("--unknown")], &mut stderr);
        assert!(matches!(result, Err(code) if code == exit::USAGE));
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "flag provided but not defined: -unknown\nUsage of sync:\n  -dry-run\n    \tshow what would be written without making changes\n  -project string\n    \tproject directory (default: current directory)\n"
        );
    }

    /// Matches the `sync_help` differential case in `scripts/rust-differential.py`
    /// byte-for-byte: `-h`/`--help` are Go flag-package special cases (bare
    /// usage, no "flag provided but not defined" line), not ordinary unknown
    /// flags. Caught by CI's `make parity-smoke`, not by the 81-case CLI tree
    /// fixture (which has no help-flag case for `sync`) — that gap is why this
    /// regressed unnoticed locally.
    #[test]
    fn help_flag_prints_bare_usage_not_unknown_flag_rejection() {
        for flag in ["-h", "--h", "-help", "--help"] {
            let mut stderr = Vec::new();
            let result = parse_args(&[OsString::from(flag)], &mut stderr);
            assert!(
                matches!(result, Err(code) if code == exit::USAGE),
                "flag {flag}"
            );
            assert_eq!(
                String::from_utf8(stderr).unwrap(),
                SYNC_FLAGS_USAGE,
                "flag {flag}"
            );
        }
    }
}
