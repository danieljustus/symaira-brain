//! Native `symbrain harness` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_harness::{ServerInfo, list};

use crate::go_flags::{FlagSet, Parsed};
use crate::health_probe::{HEALTH_SCHEMA_VERSION, HealthOutcome, probe_server};

const HARNESS_USAGE: &str = "symbrain harness — inspect configured AI harnesses

Usage:
  symbrain harness list [--project DIR]
  symbrain harness health [--harness NAME] [--project DIR]

The global --output table|json flag (or --json) selects the output format.
";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HarnessHealthEntry {
    pub harness: String,
    pub config: String,
    pub server: String,
    pub transport: String,
    pub healthy: bool,
    pub outcome: HealthOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_method: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HarnessHealthReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_schema_version: Option<u32>,
    pub servers: Option<Vec<HarnessHealthEntry>>,
}

/// The shipped `harness list` flag set as the Go flag package prints it.
const HARNESS_LIST_FLAGS: &str = "Usage of harness list:\n  -project string\n    \tproject directory to inspect for project-local harness config\n";
const HARNESS_HEALTH_FLAGS: &str = "Usage of harness health:\n  -harness string\n    \tonly probe servers of this harness\n  -project string\n    \tproject directory to inspect for project-local harness config\n";

/// Runs `symbrain harness`.
///
/// Mirrors Go's `cmdHarnessWithFormat`: only `list` and `health` are verbs;
/// every other first argument (including `-h`, `--help` and `help`) prints the
/// usage to stderr with the no-input exit code.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    match args.first().and_then(|verb| verb.to_str()) {
        Some("list") => run_list(&args[1..], stdout, stderr, format),
        Some("health") => run_health(&args[1..], stdout, stderr, format),
        _ => {
            let _ = write!(stderr, "{HARNESS_USAGE}");
            exit::USAGE
        }
    }
}

fn run_list(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let flags = FlagSet {
        string_flags: &["project"],
        usage: HARNESS_LIST_FLAGS,
    };
    let normalized = crate::normalize_flags(args);
    let Ok(parsed) = flags.parse(&normalized, stderr) else {
        return exit::USAGE;
    };
    if let Some(unexpected) = parsed.rest.first() {
        let _ = writeln!(
            stderr,
            "symbrain harness list: unexpected argument {}",
            format_go_quoted(unexpected)
        );
        return exit::USAGE;
    }
    let project_dir = project_flag(&parsed);

    let inventory = list(project_dir.as_deref());

    let result = match format {
        OutputFormat::Json => output::render_json(&mut *stdout, &inventory),
        OutputFormat::Table => render_inventory_table(&mut *stdout, &inventory),
    };
    if let Err(error) = result {
        let _ = writeln!(stderr, "symbrain harness list: format output: {error}");
        return exit::GENERIC;
    }

    exit::OK
}

/// Go treats an empty `-project` value as "no project directory".
fn project_flag(parsed: &Parsed) -> Option<PathBuf> {
    parsed
        .value("project")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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
    write!(stdout, "{label}\t")?;
    // Go writes filesystem strings verbatim to the table; JSON stays lossy UTF-8.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        stdout.write_all(config.raw_path.as_os_str().as_bytes())?;
    }
    #[cfg(not(unix))]
    write!(stdout, "{}", config.path)?;
    writeln!(stdout, "\t{state}\tservers={servers}")?;
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
) -> u8 {
    let flags = FlagSet {
        string_flags: &["harness", "project"],
        usage: HARNESS_HEALTH_FLAGS,
    };
    let normalized = crate::normalize_flags(args);
    let Ok(parsed) = flags.parse(&normalized, stderr) else {
        return exit::USAGE;
    };
    if let Some(unexpected) = parsed.rest.first() {
        let _ = writeln!(
            stderr,
            "symbrain harness health: unexpected argument {}",
            format_go_quoted(unexpected)
        );
        return exit::USAGE;
    }
    // Go's probeHarnessHealth filters with `name != "" && item.Name != name`,
    // so an empty --harness value (like an empty --project) means no filter.
    let harness_name = parsed
        .value("harness")
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned());
    let project_dir = project_flag(&parsed);

    let inventory = list(project_dir.as_deref());
    let mut probes = Vec::new();

    for h in &inventory.harnesses {
        if let Some(ref target_name) = harness_name
            && h.name.as_str() != target_name
        {
            continue;
        }
        let cfgs = std::iter::once(&h.global).chain(h.project.as_ref());
        for cfg in cfgs {
            for server in &cfg.servers {
                probes.push(HealthProbe {
                    harness: h.name.as_str().to_string(),
                    config: cfg.path.clone(),
                    server: server.clone(),
                });
            }
        }
    }

    let Some(mut entries) = run_probes(probes, probe_health) else {
        // Go has no recover here: an unrecovered probe panic aborts the Go
        // process with exit 2 and a goroutine trace. Rust deliberately exits 1
        // with this fixed message instead (HAR-008 intentional divergence); a
        // worker panic is an internal failure, never a reason to re-run Go.
        let _ = writeln!(stderr, "symbrain harness health: probe worker panicked");
        return exit::GENERIC;
    };

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
        health_schema_version: (!entries.is_empty()).then_some(HEALTH_SCHEMA_VERSION),
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
                    "  ✓  {:<12} {:<14} {} ({})",
                    server.harness, server.server, server.config, server.transport
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
        return exit::GENERIC;
    }

    exit::OK
}

/// Probes every server concurrently; `None` when any worker panicked.
///
/// Every worker is joined before the results are combined: a scoped thread
/// that panics and is never joined would re-panic when the scope ends.
fn run_probes<F>(probes: Vec<HealthProbe>, probe: F) -> Option<Vec<HarnessHealthEntry>>
where
    F: Fn(HealthProbe) -> HarnessHealthEntry + Sync,
{
    std::thread::scope(|scope| {
        let probe = &probe;
        let handles = probes
            .into_iter()
            .map(|item| scope.spawn(move || probe(item)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().ok())
            .collect::<Vec<_>>()
            .into_iter()
            .collect()
    })
}

struct HealthProbe {
    harness: String,
    config: String,
    server: ServerInfo,
}

fn probe_health(probe: HealthProbe) -> HarnessHealthEntry {
    let result = probe_server(&probe.server);
    HarnessHealthEntry {
        harness: probe.harness,
        config: probe.config,
        server: probe.server.name,
        transport: probe.server.transport,
        healthy: result.outcome == HealthOutcome::Healthy,
        outcome: result.outcome,
        probe_method: result.probe_method,
        latency_ms: result.latency_ms,
        error: result.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(name: &str) -> HealthProbe {
        HealthProbe {
            harness: "claude".to_owned(),
            config: "config.json".to_owned(),
            server: ServerInfo {
                name: name.to_owned(),
                ..ServerInfo::default()
            },
        }
    }

    fn healthy(probe: HealthProbe) -> HarnessHealthEntry {
        HarnessHealthEntry {
            harness: probe.harness,
            config: probe.config,
            server: probe.server.name,
            transport: probe.server.transport,
            healthy: true,
            outcome: HealthOutcome::Healthy,
            probe_method: None,
            latency_ms: None,
            error: String::new(),
        }
    }

    #[test]
    fn probes_keep_every_entry_when_no_worker_panics() {
        let entries = run_probes(vec![probe("a"), probe("b")], healthy).expect("no panic");
        let names = entries
            .iter()
            .map(|e| e.server.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn a_panicking_worker_yields_none_instead_of_unwinding() {
        let panicking = |probe: HealthProbe| -> HarnessHealthEntry {
            assert_ne!(probe.server.name, "boom", "injected probe failure");
            healthy(probe)
        };
        assert!(run_probes(vec![probe("a"), probe("boom")], panicking).is_none());
    }

    #[test]
    fn several_panicking_workers_do_not_re_panic_the_scope() {
        let all_panic = |_: HealthProbe| -> HarnessHealthEntry { panic!("injected probe failure") };
        assert!(run_probes(vec![probe("x"), probe("y"), probe("z")], all_panic).is_none());
    }
}
