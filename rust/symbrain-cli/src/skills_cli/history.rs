//! `symbrain skills history <skill> [--limit N]`.

use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::output::OutputFormat;
use symbrain_skills::vcs::{self, Commit};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "history",
    bools: &[],
    values: &["limit"],
    shorts: &[],
    usage: "Usage: symbrain skills history <skill> [--limit N] [--json]\n\n\
List the commits of a versioned library skill, newest first: revision,\n\
timestamp, operation (import, update, restore, unknown) and changed files.\n\
Read-only. --limit defaults to 20.\n",
};

#[derive(Serialize)]
struct Report<'a> {
    history: &'a [Commit],
    name: &'a str,
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
    let Some(name) = parsed.positional.first() else {
        return ops::usage(stderr, "history", "skill name is required");
    };
    let limit = match parsed.value("limit").map(str::parse::<i64>) {
        None => 20,
        Some(Ok(limit)) => limit,
        Some(Err(_)) => return ops::usage(stderr, "history", "--limit must be an integer"),
    };
    let env = Env::load();
    let dir = match ops::resolve_skill_dir(name, &env) {
        Ok(dir) => dir,
        Err(message) => return ops::fail(stderr, "history", message),
    };
    if !vcs::is_repo(&dir) {
        return ops::fail(
            stderr,
            "history",
            format!(
                "skill {name:?} is not versioned: no git repository at {}",
                dir.display()
            ),
        );
    }
    let history = match vcs::history(&dir, limit) {
        Ok(history) => history,
        Err(error) => return ops::fail(stderr, "history", format!("read history: {error}")),
    };
    let output = match format {
        OutputFormat::Json => json_line(&Report {
            history: &history,
            name,
        }),
        OutputFormat::Table => history
            .iter()
            .flat_map(|commit| {
                format!(
                    "{}\t{}\t{}\t{}\n",
                    commit.revision,
                    commit.timestamp,
                    commit.operation,
                    commit.files.join(", ")
                )
                .into_bytes()
            })
            .collect(),
    };
    finish("history", &output, stdout, stderr)
}
