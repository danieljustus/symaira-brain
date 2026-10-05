//! Fleet status and repair share the existing install policy.
use super::sync_flags;
use super::{current_project_dir, or_dash};
use crate::go_json;
use serde::Serialize;
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::{exit, output::OutputFormat};
use symbrain_skills::install::{
    self, InstallStatus, StatusKind, StatusOptions, SyncOptions, SyncResult,
};
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
struct SkillSyncReport {
    results: Vec<SyncResult>,
    dry_run: bool,
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

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match super::flags::parse("status", args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let (target, scope) = match parsed.target_scope("status", stderr) {
        Ok(values) => values,
        Err(code) => return code,
    };
    let cfg = symbrain_skills::config::load_cli();
    let (library_dir, base_dir, home_dir) = (
        cfg.library_dir,
        cfg.base_dir,
        symbrain_skills::config::home_dir(),
    );
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
        render_dir: Some(cfg.render_dir),
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

    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                // Go encodes every skills report with `json.Encoder`, so the
                // native output needs the same compact shape and HTML escaping.
                writeln!(stdout, "{}", go_json(&report))?;
            }
            OutputFormat::Table => {
                if report.installs.is_empty() {
                    writeln!(stdout, "No installed skills found.")?;
                    return Ok(());
                }
                let has_render = report
                    .installs
                    .iter()
                    .any(|row| row.render_status.is_some());
                let suffix = if has_render { "\tRENDER" } else { "" };
                writeln!(stdout, "TARGET\tSKILL\tSTATUS\tMODE\tPATH{suffix}")?;
                for st in &report.installs {
                    let mode = st.mode.as_deref().unwrap_or("-");
                    write!(
                        stdout,
                        "{}\t{}\t{}\t{}\t",
                        st.target,
                        st.name,
                        status_name(st.status),
                        mode
                    )?;
                    stdout.write_all(st.path.as_os_str().as_encoded_bytes())?;
                    if has_render {
                        write!(
                            stdout,
                            "\t{}",
                            st.render_status
                                .map_or("-", symbrain_skills::install::RenderStatus::label)
                        )?;
                    }
                    writeln!(stdout)?;
                }
            }
        }

        Ok(())
    })();
    super::report_result("status", rendered, stderr)
}
pub(super) fn sync(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let sync_flags::Flags {
        dry_run,
        target,
        scope,
    } = match sync_flags::parse(args, stderr) {
        Ok(flags) => flags,
        Err(code) => return code,
    };

    let cfg = symbrain_skills::config::load_cli();
    let (library_dir, base_dir, home_dir) = (
        cfg.library_dir,
        cfg.base_dir,
        symbrain_skills::config::home_dir(),
    );
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
        render_dir: Some(cfg.render_dir),
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

    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                writeln!(stdout, "{}", go_json(&report))?;
            }
            OutputFormat::Table => {
                if report.results.is_empty() {
                    writeln!(stdout, "Every installed skill is in sync.")?;
                } else {
                    writeln!(stdout, "TARGET\tSKILL\tACTION\tDETAIL")?;
                    for r in &report.results {
                        let detail = if r.error.is_empty() { "-" } else { &r.error };
                        writeln!(stdout, "{}\t{}\t{}\t{}", r.target, r.name, r.action, detail)?;
                    }
                }
            }
        }

        Ok(())
    })();
    super::report_result("sync", rendered, stderr)
}
