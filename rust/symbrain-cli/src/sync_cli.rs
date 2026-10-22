//! Native `symbrain sync` CLI implementation.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};
use symbrain_adapter::{AtomicFile, target_for_harness};
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_harness::{all, lookup};
use symbrain_instructions::{RawMessage, Source, clean_path};

/// Stands in for one invalid UTF-8 byte until the JSON text is final; Go's
/// encoder writes the six characters `\ufffd` for each such byte.
const INVALID_BYTE_SENTINEL: char = '\u{10FFFE}';

/// Text that may hold non-UTF-8 path bytes, printed raw and JSON-encoded the
/// way Go's `encoding/json` does (each invalid byte becomes `\ufffd`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoBytes(Vec<u8>);

impl GoBytes {
    fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(not(unix))]
        {
            Self(path.to_string_lossy().into_owned().into_bytes())
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<String> for GoBytes {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}

impl Serialize for GoBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(self.0.len());
        let mut rest = self.0.as_slice();
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let (valid, tail) = rest.split_at(error.valid_up_to());
                    text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    text.push(INVALID_BYTE_SENTINEL);
                    // Go decodes an invalid byte as a one-byte RuneError.
                    rest = &tail[1..];
                }
            }
        }
        serializer.serialize_str(&text)
    }
}

/// Outcome of syncing one instruction target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetStatus {
    /// Harness name.
    pub name: String,
    /// Absolute or relative path to the synced instruction file.
    pub path: GoBytes,
    /// Status: created, updated, unchanged, dry-run, skipped, error.
    pub status: String,
    /// Optional diagnostic or explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<GoBytes>,
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

/// Native sync covers every argument shape, including Go's flag terminator.
pub(crate) fn requires_go_fallback(_args: &[OsString]) -> bool {
    false
}

/// The raw bytes of `arg` after its first `skip` bytes (the flag prefix).
fn os_suffix(arg: &OsString, skip: usize) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        OsString::from_vec(arg.as_os_str().as_bytes()[skip..].to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(arg.to_string_lossy()[skip..].to_owned())
    }
}

fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<ParsedSyncArgs, u8> {
    let mut project_dir = PathBuf::from(".");
    let mut dry_run = false;
    let mut harnesses = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            harnesses.extend(
                args[i + 1..]
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            );
            break;
        } else if arg == "-dry-run" || arg == "--dry-run" {
            dry_run = true;
            i += 1;
        } else if let Some(value) = arg
            .strip_prefix("-dry-run=")
            .or_else(|| arg.strip_prefix("--dry-run="))
        {
            dry_run = match value {
                "1" | "t" | "T" | "TRUE" | "true" | "True" => true,
                "0" | "f" | "F" | "FALSE" | "false" | "False" => false,
                _ => {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {value:?} for -dry-run: parse error"
                    );
                    let _ = write!(stderr, "{SYNC_FLAGS_USAGE}");
                    return Err(exit::USAGE);
                }
            };
            i += 1;
        } else if arg == "-project" || arg == "--project" {
            if i + 1 >= args.len() {
                let _ = writeln!(stderr, "symbrain sync: flag needs an argument: {arg}");
                return Err(exit::USAGE);
            }
            project_dir = PathBuf::from(&args[i + 1]);
            i += 2;
        } else if let Some(prefix) = ["-project=", "--project="]
            .into_iter()
            .find(|prefix| arg.starts_with(prefix))
        {
            project_dir = PathBuf::from(os_suffix(&args[i], prefix.len()));
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
            // Go's flag package stops parsing at the first positional value.
            // normalizeFlags still folds double-dash names before `--`.
            let mut terminated = false;
            for remaining in &args[i..] {
                let value = remaining.to_string_lossy();
                if value == "--" {
                    terminated = true;
                }
                harnesses.push(if !terminated && value.starts_with("--") {
                    format!("-{}", &value[2..])
                } else {
                    value.into_owned()
                });
            }
            break;
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
                    "symbrain sync: harness: want one of: {}: unknown harness {name:?}",
                    symbrain_harness::names().join(", ")
                );
                return exit::USAGE;
            }
        }
        parsed.harnesses.clone()
    };

    // Go: an empty --project means the current directory.
    let project_dir = if parsed.project_dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parsed.project_dir.clone()
    };

    // Load instruction source
    let source = Source::new(Some(&project_dir));
    let instruction_content = match source.content() {
        Ok(content) => content,
        Err(err) => {
            // Go wraps the failure as "sync: load instructions: %w" and
            // cmdSync prefixes it again with "symbrain sync: ".
            let mut message = b"symbrain sync: sync: load instructions: ".to_vec();
            match err
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<RawMessage>())
            {
                Some(raw) => message.extend_from_slice(raw.bytes()),
                None => message.extend_from_slice(err.to_string().as_bytes()),
            }
            message.push(b'\n');
            let _ = stderr.write_all(&message);
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
                    path: GoBytes::default(),
                    status: "skipped".to_string(),
                    message: Some(format!("harness {name:?} has no instruction adapter").into()),
                });
                continue;
            }
            Err(err) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: GoBytes::default(),
                    status: "error".to_string(),
                    message: Some(format!("resolve target: {err}").into()),
                });
                continue;
            }
        };

        let target_path = match target.path(&project_dir) {
            Ok(p) => p,
            Err(err) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: GoBytes::default(),
                    status: "error".to_string(),
                    message: Some(err.to_string().into()),
                });
                continue;
            }
        };
        // Go's filepath.Join cleans the joined path lexically ("./a/../b/x"
        // becomes "b/x"), and the displayed path is that cleaned spelling.
        let target_path = clean_path(&target_path);
        let display_path = GoBytes::from_path(&target_path);

        let existing_bytes = fs::read(&target_path).unwrap_or_default();
        let rendered = match target.render(&existing_bytes, &instruction_content, &project_dir) {
            Ok(r) => r,
            Err(err) => {
                target_statuses.push(TargetStatus {
                    name: name.clone(),
                    path: display_path.clone(),
                    status: "error".to_string(),
                    message: Some(err.to_string().into()),
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
                message: Some("would update".to_string().into()),
            });
        } else {
            let existed = target_path.exists();
            // Go opens the target for writing first, then writes it; the two
            // failures are worded differently.
            let written = AtomicFile::open(&project_dir, Path::new(target.relative_path()), true)
                .map_err(|err| (&b" for write"[..], err))
                .and_then(|file| file.write(&rendered.output).map_err(|err| (&b""[..], err)));
            match written {
                Ok(()) => {
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: display_path.clone(),
                        status: if existed { "updated" } else { "created" }.to_string(),
                        message: None,
                    });
                }
                Err((suffix, err)) => {
                    // "open P for write: errno" / "write P: errno", with P raw.
                    let opening = !suffix.is_empty();
                    let mut message = if opening {
                        b"open ".to_vec()
                    } else {
                        b"write ".to_vec()
                    };
                    message.extend_from_slice(display_path.bytes());
                    message.extend_from_slice(suffix);
                    message.extend_from_slice(b": ");
                    message.extend_from_slice(symbrain_instructions::go_io_text(&err).as_bytes());
                    target_statuses.push(TargetStatus {
                        name: name.clone(),
                        path: display_path,
                        status: "error".to_string(),
                        message: Some(GoBytes(message)),
                    });
                }
            }
        }
    }

    let summary = SyncSummary {
        targets: target_statuses,
        skills: native_skill_results(&requested_harnesses, parsed.dry_run),
    };

    let has_error = summary.targets.iter().any(|t| t.status == "error")
        || summary.skills.iter().any(|s| s.status == "error");

    match format {
        OutputFormat::Json => {
            let mut rendered = Vec::new();
            let _ = symbrain_core::output::render_json(&mut rendered, &summary);
            // Each invalid UTF-8 byte is the six characters `\ufffd` in Go.
            let sentinel = INVALID_BYTE_SENTINEL.to_string().into_bytes();
            let mut rest = rendered.as_slice();
            while let Some(at) = rest
                .windows(sentinel.len())
                .position(|window| window == sentinel.as_slice())
            {
                let _ = stdout.write_all(&rest[..at]);
                let _ = stdout.write_all(b"\\ufffd");
                rest = &rest[at + sentinel.len()..];
            }
            let _ = stdout.write_all(rest);
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "Instruction targets:");
            for t in &summary.targets {
                let mut line =
                    format!("  {:<12} {}", format!("{}:", t.name), t.status).into_bytes();
                if let Some(message) = &t.message {
                    line.extend_from_slice(b" (");
                    line.extend_from_slice(message.bytes());
                    line.push(b')');
                }
                line.push(b'\n');
                let _ = stdout.write_all(&line);
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

    // Go reports a failed instruction target after the summary.
    if summary.targets.iter().any(|t| t.status == "error") {
        let _ = writeln!(
            stderr,
            "symbrain sync: sync: one or more instruction targets failed"
        );
    }

    if has_error { exit::GENERIC } else { exit::OK }
}

/// Runs the ported skills runner and maps its results onto the sync payload.
///
/// Mirrors `internal/sync/sync.go`: Go calls
/// `skillsrunner.Run(ctx, harnessNames, DefaultOptions(), dryRun)` with a
/// background context and turns each result into a summary entry, so a
/// per-target failure is reported as `status: "error"` and only affects the
/// exit code through `SkillsFailed`.
///
/// The runner resolves the same library, render and base roots the CLI does
/// (`resolve_skills_dirs`), so a native run installs into the same places Go
/// would; its defaults are the ones `config.Defaults()` produces.
fn native_skill_results(harnesses: &[String], dry_run: bool) -> Vec<SkillResult> {
    let (library_dir, base_dir, home_dir) = crate::skills_cli::resolve_skills_dirs();
    let opts = symbrain_skills::runner::Options {
        library_dir: library_dir.to_string_lossy().into_owned(),
        base_dir: base_dir.to_string_lossy().into_owned(),
        home_dir: home_dir.to_string_lossy().into_owned(),
        ..symbrain_skills::runner::Options::default()
    };
    // A render root is deliberately not set here: `DefaultOptions().RenderDir`
    // is derived from the XDG data root by the runner itself, and passing the
    // CLI's own view would duplicate that resolution.
    let mut ctx = symbrain_skills::runner::NoopContext;
    match symbrain_skills::runner::run(&mut ctx, harnesses, opts, dry_run) {
        Ok(results) => results
            .into_iter()
            .map(|result| SkillResult {
                target: result.target,
                status: result.status,
                message: result.message,
            })
            .collect(),
        Err(err) => vec![SkillResult {
            target: String::new(),
            status: "error".to_string(),
            message: Some(format!("sync: skills: {err}")),
        }],
    }
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
