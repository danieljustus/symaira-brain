use std::io::{self, Write};

use crate::profile_cli::{ProfileListEntry, ProfileShowReport};

pub fn print_usage(w: &mut dyn Write) -> io::Result<()> {
    write!(
        w,
        "symbrain profile — manage profiles\n\nUsage:\n  symbrain profile list\n  symbrain profile show <name>\n  symbrain profile add <name> [--from personal|restricted]\n  symbrain profile remove <name> [--force]\n\nUse the global --output table|json flag (or --json) for list and show.\nFlags may be written before or after the profile name.\n"
    )
}

pub fn print_list(w: &mut dyn Write, entries: &[ProfileListEntry]) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(
            w,
            "no profiles found (run `symbrain init` for examples, or `symbrain profile add`)"
        )?;
        return Ok(());
    }
    for entry in entries {
        if let Some(error) = &entry.error {
            writeln!(w, "{}\t(error: {error})\n", entry.name)?;
            continue;
        }
        writeln!(w, "{}\t{}", entry.name, entry.description)?;
        let servers = entry
            .servers
            .iter()
            .map(|server| {
                let state = if server.enabled {
                    if server.mode.is_empty() {
                        "on"
                    } else {
                        server.mode.as_str()
                    }
                } else {
                    "off"
                };
                format!("{}={state}", server.server)
            })
            .collect::<Vec<_>>();
        writeln!(w, "  {}\n", servers.join("  "))?;
    }
    Ok(())
}

pub fn print_show(w: &mut dyn Write, report: &ProfileShowReport) -> io::Result<()> {
    writeln!(w, "profile: {}", report.name)?;
    if !report.description.is_empty() {
        writeln!(w, "description: {}", report.description)?;
    }
    writeln!(w, "audit: enabled={}", report.audit.enabled)?;
    if !report.warnings.is_empty() {
        writeln!(w, "warnings:")?;
        for warning in &report.warnings {
            writeln!(w, "  - {warning}")?;
        }
    }
    writeln!(w)?;

    for server in &report.servers {
        write!(w, "{}: enabled={}", server.server, server.enabled)?;
        if !server.mode.is_empty() {
            write!(w, " mode={}", server.mode)?;
        }
        writeln!(w)?;
        if !server.tools_allow.is_empty() {
            writeln!(w, "  tools_allow: {}", server.tools_allow.join(", "))?;
        }
        if !server.tools_deny.is_empty() {
            writeln!(w, "  tools_deny:  {}", server.tools_deny.join(", "))?;
        }
        if let Some(policy) = &server.effective_policy {
            writeln!(w, "  effective exposed: {}", join_or_none(&policy.exposed))?;
            writeln!(w, "  effective hidden:  {}", join_or_none(&policy.hidden))?;
        } else if !server.note.is_empty() {
            writeln!(w, "  note: {}", server.note)?;
        }
        writeln!(w)?;
    }
    Ok(())
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}
