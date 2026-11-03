//! `symbrain skills list` (Go `cmdSkillsList`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::output::OutputFormat;
use symbrain_skills::library::list_library;
use symbrain_skills::metadata::{self, Options as MetadataOptions, Record, read_events_log};

use super::env::Env;
use super::flags::{self, Verb};
use super::{finish, json_line, or_dash};

#[derive(Debug, Serialize)]
struct Entry {
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    category: String,
    path: String,
    #[serde(flatten)]
    record: Record,
}

#[derive(Debug, Serialize)]
struct Report {
    skills: Vec<Entry>,
    category_counts: BTreeMap<String, usize>,
    issues: Vec<symbrain_skills::Issue>,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    // `-target` and `-scope` are parsed and ignored by Go's list.
    if let Err(code) = flags::parse(Verb::List, args, stderr) {
        return code;
    }
    let env = Env::load();
    // `os.ReadDir` resolves the raw configured path (no lexical clean).
    let (entries, issues) = list_library(&env.cfg.library_dir);
    let options = MetadataOptions {
        events: read_events_log(&env.log_path),
        log_path: env.log_path.clone(),
        home_dir: env.home_dir.clone(),
        // Go's list scan carries no project directory; user scope ignores it.
        project_dir: None,
        scope: "user".to_owned(),
    };

    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut skills = Vec::with_capacity(entries.len());
    for entry in entries {
        if !entry.category.is_empty() {
            *category_counts.entry(entry.category.clone()).or_insert(0) += 1;
        }
        let record = metadata::collect(std::path::Path::new(&entry.path), &entry.name, &options);
        skills.push(Entry {
            name: entry.name,
            description: entry.description,
            category: entry.category,
            path: entry.path,
            record,
        });
    }
    let report = Report {
        skills,
        category_counts,
        issues,
    };

    let output = match format {
        OutputFormat::Json => json_line(&report),
        OutputFormat::Table => table(&report),
    };
    finish("list", &output, stdout, stderr)
}

fn table(report: &Report) -> Vec<u8> {
    if report.skills.is_empty() {
        return b"No skills in the library.\n".to_vec();
    }
    let mut text = String::from("NAME\tCATEGORY\tINSTALLS\tDESCRIPTION\n");
    for skill in &report.skills {
        let mut targets = skill
            .record
            .installs
            .iter()
            .map(|install| install.target.as_str())
            .collect::<Vec<_>>();
        targets.sort_unstable();
        let installed = if targets.is_empty() {
            "-".to_owned()
        } else {
            targets.join(",")
        };
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            skill.name,
            or_dash(&skill.category),
            installed,
            skill.description.replace(['\t', '\r', '\n'], " ")
        ));
    }
    text.into_bytes()
}
