//! Native `symbrain harness` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use symbrain_broker::{Client, Options};
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_harness::list;

const HARNESS_USAGE: &str = "symbrain harness — inspect configured AI harnesses

Usage:
  symbrain harness list [--project DIR]
  symbrain harness health [--harness NAME] [--project DIR]

The global --output table|json flag (or --json) selects the output format.
";

#[derive(Debug, Clone, Serialize)]
pub struct HarnessHealthEntry {
    pub harness: String,
    pub config: String,
    pub server: String,
    pub transport: String,
    pub healthy: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessHealthReport {
    pub servers: Option<Vec<HarnessHealthEntry>>,
}

/// Runs `symbrain harness`.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    if args.is_empty() {
        let _ = write!(stderr, "{HARNESS_USAGE}");
        return Some(exit::USAGE);
    }

    let verb = args[0].to_string_lossy();
    let rest = &args[1..];

    match verb.as_ref() {
        "-h" | "--help" | "help" => {
            let _ = write!(stdout, "{HARNESS_USAGE}");
            Some(exit::OK)
        }
        "list" => run_list(rest, stdout, stderr, format),
        "health" => run_health(rest, stdout, stderr, format),
        _ => {
            let _ = write!(stderr, "{HARNESS_USAGE}");
            Some(exit::USAGE)
        }
    }
}

fn run_list(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    let mut project_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-project" || arg == "--project" {
            if i + 1 < args.len() {
                project_dir = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if let Some(val) = arg
            .strip_prefix("-project=")
            .or_else(|| arg.strip_prefix("--project="))
        {
            project_dir = Some(PathBuf::from(val));
            i += 1;
            continue;
        } else {
            let _ = writeln!(stderr, "symbrain harness list: unexpected argument {arg:?}");
            return Some(exit::USAGE);
        }
        i += 1;
    }

    let inventory = list(project_dir.as_deref());
    if inventory.harnesses.iter().any(|harness| {
        harness.global.error.is_some()
            || harness
                .project
                .as_ref()
                .is_some_and(|project| project.error.is_some())
    }) {
        return None;
    }

    let result = match format {
        OutputFormat::Json => output::render_json(&mut *stdout, &inventory),
        OutputFormat::Table => render_inventory_table(&mut *stdout, &inventory),
    };
    if let Err(error) = result {
        let _ = writeln!(stderr, "symbrain harness list: format output: {error}");
        return Some(exit::GENERIC);
    }

    Some(exit::OK)
}

fn render_inventory_table(
    stdout: &mut dyn Write,
    inventory: &symbrain_harness::Inventory,
) -> std::io::Result<()> {
    for harness in &inventory.harnesses {
        writeln!(
            stdout,
            "{}\t{}",
            harness.name.as_str(),
            harness.display_name
        )?;
        render_config_table(stdout, "  global", &harness.global)?;
        if let Some(project) = &harness.project {
            render_config_table(stdout, "  project", project)?;
        }
        writeln!(stdout)?;
    }
    Ok(())
}

fn render_config_table(
    stdout: &mut dyn Write,
    label: &str,
    config: &symbrain_harness::ConfigInventory,
) -> std::io::Result<()> {
    let state = if config.error.is_some() {
        "invalid"
    } else if config.parsed {
        "parsed"
    } else if config.exists {
        "unparsed"
    } else {
        "missing"
    };
    let servers = if config.servers.is_empty() {
        "(none)".to_owned()
    } else {
        config
            .servers
            .iter()
            .map(|server| format!("{}[{}]", server.name, server.transport))
            .collect::<Vec<_>>()
            .join(",")
    };
    writeln!(
        stdout,
        "{label}\t{}\t{state}\tservers={servers}",
        config.path
    )?;
    if let Some(error) = &config.error {
        writeln!(stdout, "    error: {error}")?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_health(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    let mut harness_name: Option<String> = None;
    let mut project_dir: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-harness" || arg == "--harness" {
            let value = args.get(i + 1)?;
            harness_name = Some(value.to_string_lossy().into_owned());
            i += 2;
        } else if let Some(val) = arg
            .strip_prefix("-harness=")
            .or_else(|| arg.strip_prefix("--harness="))
        {
            harness_name = Some(val.to_string());
            i += 1;
        } else if arg == "-project" || arg == "--project" {
            let value = args.get(i + 1)?;
            project_dir = Some(PathBuf::from(value));
            i += 2;
        } else {
            let val = arg
                .strip_prefix("-project=")
                .or_else(|| arg.strip_prefix("--project="))?;
            project_dir = Some(PathBuf::from(val));
            i += 1;
        }
    }

    let inventory = list(project_dir.as_deref());
    if inventory.harnesses.iter().any(|harness| {
        harness.global.error.is_some()
            || harness
                .project
                .as_ref()
                .is_some_and(|project| project.error.is_some())
    }) {
        return None;
    }

    let mut entries = Vec::new();

    for h in &inventory.harnesses {
        if let Some(ref target_name) = harness_name
            && h.name.as_str() != target_name
        {
            continue;
        }

        let cfgs = std::iter::once(&h.global).chain(h.project.as_ref());
        for cfg in cfgs {
            for s in &cfg.servers {
                if s.transport != "stdio" || s.command.is_empty() {
                    entries.push(HarnessHealthEntry {
                        harness: h.name.as_str().to_string(),
                        config: cfg.path.clone(),
                        server: s.name.clone(),
                        transport: s.transport.clone(),
                        healthy: false,
                        error: format!("not probed: {} transport is not stdio", s.transport),
                    });
                } else {
                    let path = symbrain_broker::discover(&s.command, "").ok()?;
                    let client = Client::spawn(
                        &path,
                        Options {
                            args: s.args.clone(),
                            env: None,
                            capture_stderr: true,
                        },
                    )
                    .ok()?;
                    client.initialize(Duration::from_secs(5)).ok()?;
                    entries.push(HarnessHealthEntry {
                        harness: h.name.as_str().to_string(),
                        config: cfg.path.clone(),
                        server: s.name.clone(),
                        transport: s.transport.clone(),
                        healthy: true,
                        error: String::new(),
                    });
                }
            }
        }
    }

    entries.sort_by(|left, right| {
        (
            left.harness.as_str(),
            left.server.as_str(),
            left.config.as_str(),
        )
            .cmp(&(
                right.harness.as_str(),
                right.server.as_str(),
                right.config.as_str(),
            ))
    });
    let report = HarnessHealthReport {
        servers: (!entries.is_empty()).then_some(entries),
    };

    let result = output::render(&mut *stdout, format, &report, |w| {
        let Some(servers) = report.servers.as_ref() else {
            return writeln!(w, "no MCP servers found");
        };
        for server in servers {
            if server.healthy {
                writeln!(
                    w,
                    "  ✓  {:<12} {:<14} {} (stdio)",
                    server.harness, server.server, server.config
                )?;
            } else {
                writeln!(
                    w,
                    "  ✗  {:<12} {:<14} {}: {}",
                    server.harness, server.server, server.config, server.error
                )?;
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        let _ = writeln!(stderr, "symbrain harness health: format output: {error}");
        return Some(exit::GENERIC);
    }

    Some(exit::OK)
}
