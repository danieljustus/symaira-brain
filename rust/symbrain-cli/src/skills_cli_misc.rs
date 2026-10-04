//! Targets, lifecycle log, and path diagnostics.
use super::current_project_dir;
use crate::go_json;
use serde::Serialize;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use symbrain_core::{exit, output::OutputFormat};
use symbrain_skills::install;
use symbrain_skills::targets_status::{
    StatusOptions as TargetStatusOptions, TargetStatus, list_status,
};
#[derive(Debug, Serialize)]
struct SkillTargetsReport {
    targets: Vec<TargetStatus>,
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
pub(super) fn targets(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match super::flags::parse("targets", args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let scope = match parsed.scope("targets", stderr) {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    let home = symbrain_skills::config::home_dir();
    let options = TargetStatusOptions {
        home_dir: home,
        project_dir: Some(current_project_dir()),
        scope,
    };
    let report = SkillTargetsReport {
        targets: list_status(&options),
    };

    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                writeln!(stdout, "{}", go_json(&report))?;
            }
            OutputFormat::Table => {
                writeln!(stdout, "TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT")?;
                for target in &report.targets {
                    writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}\t{}",
                        target.target,
                        target.installed,
                        target.managed_skills_count,
                        target.unmanaged_skills_count,
                        target.effective_skill_root
                    )?;
                }
            }
        }

        Ok(())
    })();
    super::report_result("targets", rendered, stderr)
}

pub(super) fn log(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match super::flags::parse("log", args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let path = symbrain_skills::config::events_path();
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
    // Event JSON strings contain Go's repaired Unicode. Invalid raw argv is
    // still distinct from a legitimate U+FFFD string and must never match it.
    if parsed.invalid_filter() {
        records.clear();
    }
    records.sort_by(|left, right| right.ts.cmp(&left.ts));
    if parsed.limit > 0 {
        records.truncate(usize::try_from(parsed.limit).unwrap_or(usize::MAX));
    }
    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                writeln!(stdout, "{}", go_json(&records))?;
            }
            OutputFormat::Table => {
                if records.is_empty() {
                    writeln!(stdout, "No recorded skill operations.")?;
                } else {
                    writeln!(stdout, "WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME")?;
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
                        writeln!(
                            stdout,
                            "{}\t{}\t{}\t{}\t{}",
                            event.ts, event.event, skill, target, event.outcome
                        )?;
                    }
                }
            }
        }
        Ok(())
    })();
    super::report_result("log", rendered, stderr)
}
pub(super) fn doctor(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if let Err(code) = super::flags::parse("doctor", args, stderr) {
        return code;
    }
    let (report, pairs) = skills_doctor_report();

    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                writeln!(stdout, "{}", go_json(&report))?;
            }
            OutputFormat::Table => {
                for (name, value) in pairs {
                    writeln!(stdout, "{name:<11} {value}")?;
                }
            }
        }

        Ok(())
    })();
    super::report_result("doctor", rendered, stderr)
}

fn skills_doctor_report() -> (SkillsDoctorReport, [(&'static str, String); 9]) {
    let home = symbrain_skills::config::home_dir();
    let project_dir = current_project_dir();
    let config_path = skills_config_path();
    let cfg = symbrain_skills::config::load_cli();
    let profiles_dir = cfg.profiles_dir.clone();
    let log_path = symbrain_skills::config::events_path();
    let config = SkillsDoctorConfig {
        library_dir: cfg.library_dir.display().to_string(),
        render_dir: cfg.render_dir.display().to_string(),
        cache_dir: cfg.cache_dir.display().to_string(),
        profiles_dir: cfg.profiles_dir.display().to_string(),
        base_dir: cfg.base_dir.display().to_string(),
        targets: None,
        vcs: SkillsDoctorVcs {
            enabled: cfg.vcs.enabled,
        },
    };
    let targets = symbrain_skills::default_targets()
        .into_iter()
        .filter_map(|target| {
            let user = symbrain_skills::skill_root(&target, &home, None, "user")?;
            let project =
                symbrain_skills::skill_root(&target, &home, Some(&project_dir), "project")?;
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
    symbrain_skills::config::config_path()
}
