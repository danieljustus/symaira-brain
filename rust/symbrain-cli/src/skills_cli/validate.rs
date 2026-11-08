//! `symbrain skills validate [skill] [--strict]`.

use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::{Issue, Resource, load_bundle, validate};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "validate",
    bools: &["strict"],
    values: &[],
    shorts: &[],
    usage: "Usage: symbrain skills validate [skill-dir|library-skill] [--strict] [--json]\n\n\
Validate a skill directory or library skill. Exits non-zero when errors exist;\n\
--strict also fails on warnings.\n",
};

#[derive(Serialize)]
struct Report {
    issues: Vec<Issue>,
    resources: Vec<Resource>,
    valid: bool,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match opargs::parse(&SPEC, args, stdout, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let Some(skill) = parsed.positional.first() else {
        return ops::usage(stderr, "validate", "skill-dir is required");
    };
    let env = Env::load();
    let path = match ops::resolve_skill_dir(skill, &env) {
        Ok(path) => path,
        Err(message) => return ops::fail(stderr, "validate", format!("load skill: {message}")),
    };
    let bundle = match load_bundle(&path) {
        Ok(bundle) => bundle,
        Err(error) => return ops::fail(stderr, "validate", format!("load skill: {error}")),
    };
    let issues = validate(&bundle);
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .count();
    let warnings = issues.len() - errors;
    let valid = errors == 0 && !(parsed.flag("strict") && warnings > 0);
    if errors > 0 {
        ops::record(
            &env,
            "validate_failure",
            &bundle.frontmatter.name,
            "",
            &path,
            "validation failed",
        );
    }
    let output = match format {
        OutputFormat::Json => json_line(&Report {
            resources: bundle.resources.clone(),
            issues,
            valid,
        }),
        OutputFormat::Table if issues.is_empty() => b"valid\n".to_vec(),
        OutputFormat::Table => issues
            .iter()
            .flat_map(|issue| {
                format!("{}\t{}\t{}\n", issue.severity, issue.code, issue.message).into_bytes()
            })
            .collect(),
    };
    let written = finish("validate", &output, stdout, stderr);
    if written != exit::OK {
        return written;
    }
    if valid {
        exit::OK
    } else {
        let _ = writeln!(stderr, "symbrain skills validate: validation failed");
        exit::GENERIC
    }
}
