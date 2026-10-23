//! Native implementation of the `symbrain guard scan` inventory command.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Write};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[path = "guard_paths.rs"]
mod paths;
#[cfg(test)]
pub(crate) use paths::clean_native_path;
pub(crate) use paths::join_native_path;

use serde::Serialize;
use symbrain_core::exit;

pub(crate) mod guard_scan_config;
pub(crate) mod guard_scan_json;
use guard_scan_config::parse_config;

const USAGE: &str = "Usage:\n  symguard scan [--format table|json]\n\nDiscovers MCP servers across supported AI clients (hermes, claude-desktop,\ncursor, vscode, opencode). The inventory is written to stdout; findings —\nclients or entries that could not be mapped — are written to stderr.\n";

#[derive(Clone, Debug, Serialize)]
struct Inventory {
    servers: Vec<ServerView>,
}

#[derive(Clone, Debug, Serialize)]
struct ServerView {
    name: String,
    client: String,
    command: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
    transport: String,
}

impl ServerView {
    fn line(&self) -> String {
        let mut line = format!("{} ({}/{})", self.name, self.client, self.transport);
        if !self.command.is_empty() {
            let _ = write!(line, " → {}", self.command);
        }
        if !self.args.is_empty() {
            let _ = write!(line, " {}", self.args.join(" "));
        }
        line
    }
}

#[derive(Clone, Debug)]
struct Server {
    name: String,
    client: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    transport: String,
}

#[derive(Clone, Debug)]
struct Finding {
    client: String,
    path: PathBuf,
    status: &'static str,
    message: String,
}

/// A client/key pair naming one of `discovery.DiscoverAll()`'s known MCP
/// client config sources (`guard/internal/discovery/discovery.go`, via
/// `mcpcfgkit.DefaultSources()`). Exposed crate-wide so `guard_doctor`'s
/// empty-machine gate can probe the exact same path list scan already uses,
/// instead of maintaining a second copy of it.
#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub(crate) client: &'static str,
    pub(crate) key: &'static str,
}

pub(crate) const SOURCES: [Source; 5] = [
    Source {
        client: "hermes",
        key: "mcpServers",
    },
    Source {
        client: "cursor",
        key: "mcpServers",
    },
    Source {
        client: "vscode",
        key: "mcpServers",
    },
    Source {
        client: "opencode",
        key: "mcp",
    },
    Source {
        client: "claude-desktop",
        key: "mcpServers",
    },
];

pub fn run(args: &[std::ffi::OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let format = match parse_flags(args, stdout, stderr) {
        Ok(format) => format,
        Err(code) => return code,
    };
    let format = format.unwrap_or_else(|| {
        if io::stdout().is_terminal() {
            "table".to_owned()
        } else {
            "json".to_owned()
        }
    });
    if format != "table" && format != "json" {
        let _ = writeln!(
            stderr,
            "scan: unsupported format {format:?} (supported: table, json)"
        );
        return exit::GENERIC;
    }

    let (mut servers, findings) = scan_all();
    servers.sort_by(|a, b| a.client.cmp(&b.client).then_with(|| a.name.cmp(&b.name)));
    let inventory = Inventory {
        servers: servers.into_iter().map(view).collect(),
    };
    let output = if format == "json" {
        serde_json::to_string_pretty(&inventory).map(|text| go_html_escape(&text) + "\n")
    } else {
        let mut text = format!("Discovered {} MCP server(s)\n", inventory.servers.len());
        for server in &inventory.servers {
            let _ = writeln!(text, "  {}", server.line());
        }
        if text.ends_with('\n') {
            text.pop();
        }
        text.push('\n');
        Ok(text)
    };
    match output {
        Ok(text) if stdout.write_all(text.as_bytes()).is_ok() => {}
        Ok(_) => {
            let _ = writeln!(stderr, "scan: write output: output error");
            return exit::GENERIC;
        }
        Err(error) => {
            let _ = writeln!(stderr, "scan: write output: {error}");
            return exit::GENERIC;
        }
    }
    write_findings(stderr, &findings);
    exit::OK
}

fn parse_flags(
    args: &[std::ffi::OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<Option<String>, u8> {
    let mut format = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--format" {
            i += 1;
            let Some(value) = args.get(i) else {
                let _ = writeln!(stderr, "scan: --format requires a value (table or json)");
                return Err(exit::GENERIC);
            };
            let value = value.to_string_lossy();
            format = (!value.is_empty()).then(|| value.into_owned());
        } else if let Some(value) = arg.strip_prefix("--format=") {
            format = (!value.is_empty()).then(|| value.to_owned());
        } else if arg == "--help" || arg == "-h" {
            let _ = stdout.write_all(USAGE.as_bytes());
            return Err(exit::OK);
        } else {
            let _ = writeln!(stderr, "scan: unknown argument {arg:?}");
            let _ = stderr.write_all(USAGE.as_bytes());
            return Err(exit::GENERIC);
        }
        i += 1;
    }
    Ok(format)
}

fn scan_all() -> (Vec<Server>, Vec<Finding>) {
    let mut servers = Vec::new();
    let mut findings = Vec::new();
    for source in SOURCES {
        let path = source_path(source);
        match read_source(&path) {
            Ok(data) => match parse_config(&data, source.key) {
                Ok(entries) => {
                    for (name, entry) in entries {
                        let command = entry.command_or_url();
                        if command.is_empty() {
                            findings.push(Finding {
                                client: source.client.to_owned(),
                                path: path.clone(),
                                status: "unsupported",
                                message: format!("server {name:?} is missing both command and url"),
                            });
                            continue;
                        }
                        if source.client == "opencode"
                            && !entry.kind.is_empty()
                            && !entry.kind.eq_ignore_ascii_case("local")
                            && !entry.kind.eq_ignore_ascii_case("remote")
                        {
                            findings.push(Finding {
                                client: source.client.to_owned(),
                                path: path.clone(),
                                status: "approximate",
                                message: format!(
                                    "server {name:?} has unknown type {typ:?}, treated as local",
                                    typ = entry.kind
                                ),
                            });
                        }
                        let env = entry.merged_env();
                        let transport = entry.transport();
                        servers.push(Server {
                            name,
                            client: source.client.to_owned(),
                            command,
                            args: entry.args,
                            env,
                            transport,
                        });
                    }
                }
                Err(error) => findings.push(Finding {
                    client: source.client.to_owned(),
                    path,
                    status: "unsupported",
                    message: error,
                }),
            },
            Err(failure) if missing_source_is_silent(&failure.error) => findings.push(Finding {
                client: source.client.to_owned(),
                path,
                status: "unsupported",
                message: "config file not found".to_owned(),
            }),
            Err(failure) => findings.push(Finding {
                client: source.client.to_owned(),
                message: read_error_message(&path, &failure),
                path,
                status: "unsupported",
            }),
        }
    }
    (servers, findings)
}

/// Mirrors the Go adapter's string-based missing-file check. On Windows,
/// `ERROR_FILE_NOT_FOUND` (2) is recognized by that check, but
/// `ERROR_PATH_NOT_FOUND` (3) is not; on Unix all `NotFound` errors are skipped.
pub(crate) fn missing_source_is_silent(error: &io::Error) -> bool {
    if error.kind() != io::ErrorKind::NotFound {
        return false;
    }
    #[cfg(windows)]
    {
        error.raw_os_error() != Some(3)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// A failed `os.ReadFile`: Go reports the failing step (`open` or `read`) in its `PathError`.
pub(crate) struct ReadFailure {
    pub(crate) op: &'static str,
    pub(crate) error: io::Error,
}

/// `os.ReadFile`: opening a directory succeeds and the first read then fails,
/// so Go names the operation `read` there and `open` for every open failure.
pub(crate) fn read_source(path: &std::path::Path) -> Result<Vec<u8>, ReadFailure> {
    use std::io::Read as _;
    let mut file = fs::File::open(path).map_err(|error| ReadFailure { op: "open", error })?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|error| ReadFailure { op: "read", error })?;
    Ok(data)
}

/// Formats the read diagnostic emitted by Go's os.ReadFile `PathError`.
pub(crate) fn read_error_message(path: &std::path::Path, failure: &ReadFailure) -> String {
    format!(
        "read failed: {} {}: {}",
        failure.op,
        path.display(),
        go_errno_text(&failure.error)
    )
}

/// The platform error text without Rust's ` (os error N)` suffix. Go prints its
/// own syscall text instead, so wording may differ (documented deviation, #770).
pub(crate) fn io_detail(error: &io::Error) -> String {
    let detail = error.to_string();
    error
        .raw_os_error()
        .and_then(|code| {
            detail
                .strip_suffix(&format!(" (os error {code})"))
                .map(str::to_owned)
        })
        .unwrap_or(detail)
}

/// Go's errno table on Unix is the C library text with a lower-case first
/// letter (`is a directory`); `os.ReadFile` diagnostics use it. Windows wording
/// stays platform text.
fn go_errno_text(error: &io::Error) -> String {
    let detail = io_detail(error);
    if cfg!(unix) && error.raw_os_error().is_some() {
        let mut chars = detail.chars();
        if let Some(first) = chars.next() {
            return first.to_lowercase().chain(chars).collect();
        }
    }
    detail
}

/// Go's `encoding/json` encoder escapes HTML-significant characters and the
/// JavaScript line separators by default; they only occur inside strings.
fn go_html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn source_path(source: Source) -> PathBuf {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let parts: &[&str] = match source.client {
        "hermes" => &[".config", "hermes", "config.json"],
        "cursor" => &[".cursor", "mcp.json"],
        "vscode" => &[".vscode", "mcp.json"],
        "opencode" => &[".config", "opencode", "config.json"],
        "claude-desktop" if cfg!(target_os = "macos") => &[
            "Library",
            "Application Support",
            "Claude",
            "claude_desktop_config.json",
        ],
        "claude-desktop" => {
            let base = env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map_or_else(|| join_native_path(&home, &[".config"]), PathBuf::from);
            return join_native_path(&base, &["claude", "claude_desktop_config.json"]);
        }
        _ => return PathBuf::new(),
    };
    join_native_path(&home, parts)
}

fn view(server: Server) -> ServerView {
    ServerView {
        name: server.name,
        client: server.client,
        command: server.command,
        args: server.args,
        env: server
            .env
            .into_keys()
            .map(|key| (key, "REDACTED".to_owned()))
            .collect(),
        transport: server.transport,
    }
}

fn write_findings(stderr: &mut dyn Write, findings: &[Finding]) {
    if findings.is_empty() {
        return;
    }
    let _ = writeln!(stderr, "symguard scan: {} finding(s)", findings.len());
    for finding in findings {
        let _ = writeln!(
            stderr,
            "  {} [{}] {}: {}",
            finding.status,
            finding.client,
            finding.path.display(),
            finding.message
        );
    }
}

#[cfg(test)]
#[path = "guard_scan_tests.rs"]
mod tests;
