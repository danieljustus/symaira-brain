//! Native memory read contracts.

use super::{MEMORY_LIST_USAGE, OsString, OutputFormat, Write, exit, flags, open_store};

pub(super) fn run_list(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{MEMORY_LIST_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "list", stderr) else {
        return exit::USAGE;
    };
    if let Some(argument) = parsed.positional.first() {
        let _ = writeln!(
            stderr,
            "symbrain memory list: unexpected argument {}",
            symbrain_core::config::format_go_quoted(argument)
        );
        return exit::USAGE;
    }
    let scope = parsed.text("scope");
    let limit = if parsed.limit <= 0 {
        100
    } else {
        parsed.limit.min(1000)
    };
    let store = match open_store(stderr, "list", Some(&parsed.raw("db"))) {
        Ok(store) => store,
        Err(code) => return code,
    };

    let rows = match store.list_lite(&scope, usize::try_from(limit).unwrap_or(1000)) {
        Ok(rows) => rows,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory list: list memories: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let rendered = rows
                .iter()
                .map(symbrain_memory::MemoryListRow::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(
                stdout,
                "{}",
                super::search::escape_html(&format!("[{rendered}]"))
            );
        }
        OutputFormat::Table => {
            if rows.is_empty() {
                let _ = writeln!(stdout, "No memories found.");
            } else {
                let _ = writeln!(stdout, "ID\tSCOPE\tCREATED\tCONTENT");
                for row in &rows {
                    let created = row
                        .created_at
                        .map(|time| {
                            time.with_timezone(&chrono::Utc)
                                .format("%Y-%m-%dT%H:%M:%SZ")
                                .to_string()
                        })
                        .unwrap_or_default();
                    write_row(
                        stdout,
                        &[&row.id, &row.scope, &created, &table_content(&row.content)],
                    );
                }
            }
        }
    }

    exit::OK
}

/// Writes one tab-separated table row; stored bytes that are not valid
/// UTF-8 are written back unchanged, like the shipped table writer.
pub(super) fn write_row(out: &mut dyn Write, fields: &[&str]) {
    let mut line = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            line.push(b'\t');
        }
        line.extend_from_slice(&symbrain_memory::raw_bytes(field));
    }
    line.push(b'\n');
    let _ = out.write_all(&line);
}

/// Collapses tab, carriage return and newline so one memory stays on one row.
pub(super) fn table_content(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}
