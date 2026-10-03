//! Native memory rules contracts.

use super::{
    MEMORY_RULES_USAGE, OsString, OutputFormat, Write, exit, flags, open_store, table_content,
};

pub(super) fn run_rules(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{MEMORY_RULES_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "rules", stderr) else {
        return exit::USAGE;
    };
    if let Some(argument) = parsed.positional.first() {
        let _ = writeln!(
            stderr,
            "symbrain memory rules: unexpected argument {}",
            symbrain_core::config::format_go_quoted(argument)
        );
        return exit::USAGE;
    }
    let scope = parsed.text("scope");
    let store = match open_store(stderr, Some(&parsed.raw("db"))) {
        Ok(store) => store,
        Err(code) => return code,
    };

    let rules = match store.list_rules(&scope) {
        Ok(rules) => rules,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory rules: list rules: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let rendered = rules
                .iter()
                .map(symbrain_memory::RuleRow::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(
                stdout,
                "{}",
                super::search::escape_html(&format!("[{rendered}]"))
            );
        }
        OutputFormat::Table => {
            if rules.is_empty() {
                let _ = writeln!(stdout, "No rules found.");
            } else {
                let _ = writeln!(stdout, "ID\tSCOPE\tCREATED\tCONTENT");
                for rule in &rules {
                    let created = rule
                        .created_at
                        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                        .unwrap_or_default();
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        rule.id,
                        rule.scope,
                        created,
                        table_content(&rule.content)
                    );
                }
            }
        }
    }

    exit::OK
}
