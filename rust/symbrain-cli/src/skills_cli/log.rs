//! `symbrain skills log` (Go `cmdSkillsLog`).

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::gotext::path_error;
use symbrain_skills::install::{self, OperationEvent};

use super::env::Env;
use super::flags::{self, Verb, bytes};
use super::{finish, json_line, or_dash};

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match flags::parse(Verb::Log, args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let env = Env::load();
    // Events hold valid UTF-8 only (Go repairs bytes on decode), so a filter
    // with invalid bytes can never equal an event field.
    let filter = |raw: &[u8]| -> Option<Option<String>> {
        let trimmed = bytes::trim_target(raw);
        if trimmed.is_empty() {
            return Some(None);
        }
        std::str::from_utf8(trimmed)
            .ok()
            .map(|text| Some(text.to_owned()))
    };
    let (skill, target) = (filter(&parsed.skill), filter(&parsed.target));
    let mut records = match install::read_events_go_exact(
        &env.log_path,
        skill.clone().flatten().as_deref(),
        target.clone().flatten().as_deref(),
    ) {
        Ok(records) => records,
        Err(failure) => {
            let _ = writeln!(
                stderr,
                "symbrain skills log: read operation log: {}",
                path_error(failure.op, &failure.path, &failure.source)
            );
            return exit::GENERIC;
        }
    };
    if skill.is_none() || target.is_none() {
        records.clear();
    }
    records.sort_by(|left, right| right.ts.cmp(&left.ts));
    if parsed.limit > 0 {
        records.truncate(usize::try_from(parsed.limit).unwrap_or(usize::MAX));
    }
    let output = match format {
        OutputFormat::Json => json_line(&records),
        OutputFormat::Table => table(&records),
    };
    finish("log", &output, stdout, stderr)
}

fn table(records: &[OperationEvent]) -> Vec<u8> {
    if records.is_empty() {
        return b"No recorded skill operations.\n".to_vec();
    }
    let mut text = String::from("WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME\n");
    for event in records {
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}",
            event.ts,
            event.event,
            or_dash(&event.skill),
            or_dash(&event.target),
            event.outcome
        );
    }
    text.into_bytes()
}
