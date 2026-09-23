//! Native implementation of the `symbrain guard scan` inventory command.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use serde::Serialize;
use symbrain_core::exit;

pub(crate) mod guard_scan_config;
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
        serde_json::to_string_pretty(&inventory).map(|mut text| {
            text.push('\n');
            text
        })
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
        match fs::read(&path) {
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
            Err(error) if missing_source_is_silent(&error) => findings.push(Finding {
                client: source.client.to_owned(),
                path,
                status: "unsupported",
                message: "config file not found".to_owned(),
            }),
            Err(error) => findings.push(Finding {
                client: source.client.to_owned(),
                message: read_error_message(&path, &error),
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

/// Formats the read diagnostic emitted by Go's os.ReadFile `PathError`.
pub(crate) fn read_error_message(path: &std::path::Path, error: &io::Error) -> String {
    let mut detail = error.to_string();
    if let Some(code) = error.raw_os_error() {
        let suffix = format!(" (os error {code})");
        if let Some(without_code) = detail.strip_suffix(&suffix) {
            detail = without_code.to_owned();
        }
    }
    format!("read failed: open {}: {detail}", path.display())
}

pub(crate) fn source_path(source: Source) -> PathBuf {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    match source.client {
        "hermes" => home.join(".config/hermes/config.json"),
        "cursor" => home.join(".cursor/mcp.json"),
        "vscode" => home.join(".vscode/mcp.json"),
        "opencode" => home.join(".config/opencode/config.json"),
        "claude-desktop" if cfg!(target_os = "macos") => {
            home.join("Library/Application Support/Claude/claude_desktop_config.json")
        }
        "claude-desktop" => env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("claude/claude_desktop_config.json"),
        _ => PathBuf::new(),
    }
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
mod tests {
    use super::*;

    #[test]
    fn jsonc_preserves_urls_and_removes_comments() {
        let parsed = parse_config(
            br#"{// comment
          "mcpServers":{"s":{"url":"https://example.test/mcp"}}
        }"#,
            "mcpServers",
        )
        .expect("JSONC");
        assert_eq!(parsed["s"].transport(), "http");
        assert_eq!(parsed["s"].command_or_url(), "https://example.test/mcp");
    }

    #[test]
    fn opencode_environment_is_merged_and_redacted() {
        let parsed = parse_config(br#"{"mcp":{"s":{"type":"local","command":"run","env":{"A":"old","B":"x"},"environment":{"A":"new","C":"y"}}}}"#, "mcp").expect("OpenCode");
        let view = view(Server {
            name: "s".into(),
            client: "opencode".into(),
            command: "run".into(),
            args: vec![],
            env: parsed["s"].merged_env(),
            transport: parsed["s"].transport(),
        });
        assert_eq!(
            view.env,
            BTreeMap::from([
                (String::from("A"), String::from("REDACTED")),
                (String::from("B"), String::from("REDACTED")),
                (String::from("C"), String::from("REDACTED"))
            ])
        );
    }

    #[test]
    fn empty_format_value_resolves_to_default() {
        let args = vec![std::ffi::OsString::from("--format=")];
        assert_eq!(
            parse_flags(&args, &mut Vec::new(), &mut Vec::new()),
            Ok(None)
        );
    }

    #[test]
    fn missing_source_error_classification_matches_go_platform_behavior() {
        let missing_file = io::Error::from(io::ErrorKind::NotFound);
        assert!(missing_source_is_silent(&missing_file));

        #[cfg(windows)]
        {
            assert!(missing_source_is_silent(&io::Error::from_raw_os_error(2)));
            let missing_parent = io::Error::from_raw_os_error(3);
            assert!(!missing_source_is_silent(&missing_parent));
        }
    }

    #[test]
    fn json_typed_fields_fail_instead_of_being_dropped() {
        for document in [
            br#"{"mcpServers":{"s":{"command":42}}}"#.as_slice(),
            br#"{"mcpServers":{"s":{"args":[42]}}}"#.as_slice(),
            br#"{"mcpServers":{"s":{"env":{"TOKEN":42}}}}"#.as_slice(),
        ] {
            assert!(parse_config(document, "mcpServers").is_err());
        }
    }

    #[test]
    fn yaml_environment_is_not_merged() {
        let parsed = parse_config(
            b"mcpServers:\n  s:\n    command: run\n    environment:\n      TOKEN: ignored\n",
            "mcpServers",
        )
        .expect("YAML");
        assert!(parsed["s"].env.is_empty());
    }
}
