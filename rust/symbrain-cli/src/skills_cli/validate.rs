//! `symbrain skills validate [skill] [--strict] [--targets]`.

use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::harness_findings::{Finding, evaluate_bundle};
use symbrain_skills::{Issue, Resource, load_bundle, validate};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "validate",
    bools: &["strict", "targets"],
    values: &[],
    shorts: &[],
    usage: "Usage: symbrain skills validate [skill-dir|library-skill] [--strict] [--targets] [--json]\n\n\
Validate a skill directory or library skill. Exits non-zero when errors exist;\n\
--strict also fails on warnings. --targets adds advisory per-harness delivery\n\
warnings measured on each enabled target's rendered bundle.\n",
};

#[derive(Serialize)]
struct Report {
    issues: Vec<Issue>,
    resources: Vec<Resource>,
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_findings: Option<Vec<Finding>>,
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
    let findings = parsed
        .flag("targets")
        .then(|| evaluate_bundle(&bundle, &env.disabled_targets));
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .count();
    let warnings = issues.len() - errors + findings.as_ref().map_or(0, Vec::len);
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
            target_findings: findings,
        }),
        OutputFormat::Table => table(&issues, findings.as_deref().unwrap_or_default()),
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

fn table(issues: &[Issue], findings: &[Finding]) -> Vec<u8> {
    if issues.is_empty() && findings.is_empty() {
        return b"valid\n".to_vec();
    }
    issues
        .iter()
        .cloned()
        .chain(findings.iter().map(Finding::to_issue))
        .flat_map(|issue| {
            format!("{}\t{}\t{}\n", issue.severity, issue.code, issue.message).into_bytes()
        })
        .collect()
}
