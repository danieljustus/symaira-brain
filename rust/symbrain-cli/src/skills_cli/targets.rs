//! `symbrain skills targets` (Go `cmdSkillsTargets`).

use std::ffi::{OsStr, OsString};
use std::io::Write;

use serde::Serialize;
use symbrain_core::config::format_go_quoted_bytes as quote;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::targets_status::{StatusOptions, TargetStatus, list_status};

use super::env::Env;
use super::flags::{self, Verb, bytes};
use super::{finish, json_line};

#[derive(Debug, Serialize)]
struct Report {
    targets: Vec<TargetStatus>,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match flags::parse(Verb::Targets, args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let scope = match bytes::trim_target(&parsed.scope) {
        b"" | b"user" => "user",
        b"project" => "project",
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills targets: unknown scope {} (known: user, project)",
                quote(&parsed.scope)
            );
            return exit::USAGE;
        }
    };
    let env = Env::load();
    let report = Report {
        targets: list_status(&StatusOptions {
            home_dir: env.home_dir,
            project_dir: Some(env.project_dir),
            scope: scope.to_owned(),
            disabled_targets: env.disabled_targets,
        }),
    };
    let output = match format {
        OutputFormat::Json => json_line(&report),
        OutputFormat::Table => table(&report.targets),
    };
    finish("targets", &output, stdout, stderr)
}

fn table(targets: &[TargetStatus]) -> Vec<u8> {
    let mut out = b"TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT\n".to_vec();
    for target in targets {
        out.extend_from_slice(
            format!(
                "{}\t{}\t{}\t{}\t",
                target.target,
                target.installed,
                target.managed_skills_count,
                target.unmanaged_skills_count
            )
            .as_bytes(),
        );
        out.extend_from_slice(OsStr::new(&target.effective_skill_root).as_encoded_bytes());
        out.push(b'\n');
    }
    out
}
