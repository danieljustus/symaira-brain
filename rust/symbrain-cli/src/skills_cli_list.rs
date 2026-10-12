//! Populated CLI list uses the shared lifecycle metadata collector.
use super::or_dash;
use crate::go_json;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::output::OutputFormat;
use symbrain_skills::library::list_library;
use symbrain_skills::metadata::{self, Options as MetadataOptions, Record, read_events_log};
#[derive(Debug, Serialize)]
struct SkillListEntry {
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    category: String,
    #[serde(serialize_with = "symbrain_skills::text::serialize_path")]
    path: std::path::PathBuf,
    #[serde(flatten)]
    record: Record,
}

#[derive(Debug, Serialize)]
struct SkillListReport {
    skills: Vec<SkillListEntry>,
    category_counts: BTreeMap<String, usize>,
    issues: Vec<symbrain_skills::Issue>,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if let Err(code) = super::flags::parse("list", args, stderr) {
        return code;
    }
    let cfg = symbrain_skills::config::load_cli();
    let library_dir = cfg.library_dir;
    let (entries, issues) = list_library(&library_dir);

    let home = symbrain_skills::config::home_dir();
    let log_path = symbrain_skills::config::events_path();
    let metadata_options = MetadataOptions {
        events: read_events_log(&log_path),
        log_path,
        home_dir: home,
        // Go's list scan carries no project directory; user scope ignores it
        // anyway, and the marker fallback must resolve the same roots.
        project_dir: None,
        scope: "user".to_owned(),
    };

    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut skills = Vec::with_capacity(entries.len());
    for entry in entries {
        if !entry.category.is_empty() {
            *category_counts.entry(entry.category.clone()).or_insert(0) += 1;
        }
        let record = metadata::collect(&entry.path, &entry.name, &metadata_options);
        skills.push(SkillListEntry {
            name: entry.name,
            description: entry.description,
            category: entry.category,
            path: entry.path,
            record,
        });
    }

    let report = SkillListReport {
        skills,
        category_counts,
        issues,
    };

    let rendered = (|| -> std::io::Result<()> {
        match format {
            OutputFormat::Json => {
                writeln!(stdout, "{}", go_json(&report))?;
            }
            OutputFormat::Table => {
                if report.skills.is_empty() {
                    writeln!(stdout, "No skills in the library.")?;
                } else {
                    writeln!(stdout, "NAME\tCATEGORY\tINSTALLS\tDESCRIPTION")?;
                    for skill in &report.skills {
                        let category = or_dash(&skill.category);
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
                        writeln!(
                            stdout,
                            "{}\t{}\t{}\t{}",
                            skill.name,
                            category,
                            installed,
                            table_content(&skill.description)
                        )?;
                    }
                }
            }
        }

        Ok(())
    })();
    super::report_result("list", rendered, stderr)
}

fn table_content(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}
