use chrono::Utc;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use symbrain_core::exit;
#[cfg(test)]
use symbrain_harness::Entry;
use symbrain_harness::{AtomicFile, ConfigLocation, Harness, HarnessError, SERVER_NAME};

const INSTALL_USAGE: &str = "Usage of install:\n  -dry-run\n    \tprint a unified diff of the change and write nothing\n  -harness string\n    \tharness to install into: claude, claude-desktop, cursor, opencode, codex, antigravity (required)\n  -keep-superseded\n    \tkeep superseded symmemory/symskills MCP entries instead of migrating them out\n  -profile string\n    \tprofile to bind this harness connection to (default: the global config's default_profile)\n  -project string\n    \tproject directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)\n";

const UNINSTALL_USAGE: &str = "Usage of uninstall:\n  -dry-run\n    \tprint a unified diff of the change and write nothing\n  -harness string\n    \tharness to remove symbrain from: claude, claude-desktop, cursor, opencode, codex, antigravity (required)\n  -project string\n    \tproject directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)\n";

#[derive(Default)]
struct InstallArgs {
    harness: String,
    profile: String,
    project: String,
    dry_run: bool,
    keep_superseded: bool,
}

#[derive(Default)]
struct UninstallArgs {
    harness: String,
    project: String,
    dry_run: bool,
}

struct ResolvedTarget {
    harness: &'static Harness,
    location: ConfigLocation,
}

pub(crate) fn run_install(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let Some(parsed) = parse_install_args(args, stderr) else {
        return exit::USAGE;
    };
    let Some(target) = resolve_harness(&parsed.harness, &parsed.project, "install", stderr) else {
        return exit::USAGE;
    };

    let profile = if parsed.profile.is_empty() {
        match default_profile() {
            Ok(profile) => profile,
            Err(error) => {
                let _ = error.write("symbrain install: ", stderr);
                return exit::USAGE;
            }
        }
    } else {
        parsed.profile.into()
    };
    if profile.is_empty() {
        let _ = writeln!(
            stderr,
            "symbrain install: no --profile given and no default_profile configured in ~/.config/symbrain/config.toml (pass --profile, or run `symbrain init`)"
        );
        return exit::NO_INPUT;
    }

    install_into(
        target.harness,
        &target.location,
        &profile,
        parsed.dry_run,
        parsed.keep_superseded,
        stdout,
        stderr,
    )
}

pub(crate) fn run_uninstall(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let Some(parsed) = parse_uninstall_args(args, stderr) else {
        return exit::USAGE;
    };
    let Some(target) = resolve_harness(&parsed.harness, &parsed.project, "uninstall", stderr)
    else {
        return exit::USAGE;
    };
    uninstall_from(
        target.harness,
        &target.location,
        parsed.dry_run,
        stdout,
        stderr,
    )
}

fn parse_install_args(args: &[OsString], stderr: &mut dyn Write) -> Option<InstallArgs> {
    let mut parsed = InstallArgs::default();
    let normalized = crate::normalize_flags(args);
    if !parse_flags(&normalized, &mut parsed, stderr, INSTALL_USAGE) {
        return None;
    }
    Some(parsed)
}

fn parse_uninstall_args(args: &[OsString], stderr: &mut dyn Write) -> Option<UninstallArgs> {
    let mut parsed = UninstallArgs::default();
    let normalized = crate::normalize_flags(args);
    if !parse_flags(&normalized, &mut parsed, stderr, UNINSTALL_USAGE) {
        return None;
    }
    Some(parsed)
}

trait FlagValues {
    fn set_value(&mut self, name: &str, value: String) -> bool;
    fn set_bool(&mut self, name: &str, value: bool) -> bool;
    fn has_value_flag(name: &str) -> bool;
    fn has_bool_flag(name: &str) -> bool;
}

impl FlagValues for InstallArgs {
    fn set_value(&mut self, name: &str, value: String) -> bool {
        match name {
            "harness" => self.harness = value,
            "profile" => self.profile = value,
            "project" => self.project = value,
            _ => return false,
        }
        true
    }

    fn set_bool(&mut self, name: &str, value: bool) -> bool {
        match name {
            "dry-run" => self.dry_run = value,
            "keep-superseded" => self.keep_superseded = value,
            _ => return false,
        }
        true
    }

    fn has_value_flag(name: &str) -> bool {
        matches!(name, "harness" | "profile" | "project")
    }

    fn has_bool_flag(name: &str) -> bool {
        matches!(name, "dry-run" | "keep-superseded")
    }
}

impl FlagValues for UninstallArgs {
    fn set_value(&mut self, name: &str, value: String) -> bool {
        match name {
            "harness" => self.harness = value,
            "project" => self.project = value,
            _ => return false,
        }
        true
    }

    fn set_bool(&mut self, name: &str, value: bool) -> bool {
        if name == "dry-run" {
            self.dry_run = value;
            true
        } else {
            false
        }
    }

    fn has_value_flag(name: &str) -> bool {
        matches!(name, "harness" | "project")
    }

    fn has_bool_flag(name: &str) -> bool {
        name == "dry-run"
    }
}

fn parse_flags<T: FlagValues>(
    args: &[OsString],
    parsed: &mut T,
    stderr: &mut dyn Write,
    usage: &str,
) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" || !arg.starts_with('-') || arg == "-" {
            // The Go flag package stops parsing at the first positional. The
            // command historically ignores those remaining positionals.
            break;
        }
        let name = arg.trim_start_matches('-');
        let (name, inline_value) = name
            .split_once('=')
            .map_or((name, None), |(name, value)| (name, Some(value)));
        if name == "h" || name == "help" {
            let _ = write!(stderr, "{usage}");
            return false;
        }
        if T::has_value_flag(name) {
            let value = if let Some(value) = inline_value {
                value.to_owned()
            } else if let Some(value) = args.get(index + 1) {
                index += 1;
                value.to_string_lossy().into_owned()
            } else {
                let _ = writeln!(stderr, "flag needs an argument: -{name}");
                let _ = write!(stderr, "{usage}");
                return false;
            };
            let _ = parsed.set_value(name, value);
        } else if T::has_bool_flag(name) {
            let value = match inline_value {
                None | Some("true") => true,
                Some("false") => false,
                Some(value) => {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {value:?} for -{name}: parse error"
                    );
                    let _ = write!(stderr, "{usage}");
                    return false;
                }
            };
            let _ = parsed.set_bool(name, value);
        } else {
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = write!(stderr, "{usage}");
            return false;
        }
        index += 1;
    }
    true
}

fn resolve_harness(
    name: &str,
    project: &str,
    command: &str,
    stderr: &mut dyn Write,
) -> Option<ResolvedTarget> {
    if name.is_empty() {
        let names = symbrain_harness::names().join(", ");
        let _ = writeln!(
            stderr,
            "symbrain {command}: --harness is required (want one of: {names})"
        );
        return None;
    }
    let harness = match symbrain_harness::lookup(name) {
        Ok(harness) => harness,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain {command}: {error}");
            return None;
        }
    };
    if !harness.supports_mcp_install() {
        let _ = writeln!(
            stderr,
            "symbrain {command}: resolve config path for {}: harness does not support MCP installation",
            harness.name
        );
        return None;
    }
    if !project.is_empty() {
        if !harness.supports_project {
            let _ = writeln!(
                stderr,
                "symbrain {command}: harness {:?} has no project-local config; omit --project",
                harness.name.as_str()
            );
            return None;
        }
        let path = harness.project_config_path(Path::new(project))?;
        return Some(ResolvedTarget {
            harness,
            location: ConfigLocation {
                path,
                trusted_root: PathBuf::from(project),
                relative_path: PathBuf::from(".mcp.json"),
            },
        });
    }
    match harness.config_location() {
        Ok(location) => Some(ResolvedTarget { harness, location }),
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain {command}: resolve config path for {}: {error}",
                harness.name
            );
            None
        }
    }
}

fn default_profile() -> Result<symbrain_core::GoText, symbrain_core::config::resolved::ConfigError>
{
    symbrain_core::config::resolved::load().map(|config| config.default_profile)
}

#[allow(clippy::too_many_lines)]
fn install_into(
    harness: &Harness,
    location: &ConfigLocation,
    profile: &symbrain_core::GoText,
    dry_run: bool,
    keep_superseded: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let (capability, snapshot) = match open_existing(location) {
        Ok(value) => value,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain install: read {}: {error}",
                location.path.display()
            );
            return exit::GENERIC;
        }
    };
    let (original, existed) = snapshot.as_ref().map_or_else(
        || (Vec::new(), false),
        |snapshot| (snapshot.bytes.clone(), true),
    );
    let mut document = if existed {
        match symbrain_harness::parse(harness, &original) {
            Ok(document) => document,
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "symbrain install: {}: {}",
                    location.path.display(),
                    format_parse_error(harness, &original, &error)
                );
                return exit::NO_INPUT;
            }
        }
    } else {
        symbrain_harness::empty(harness)
    };

    let mut migrated = Vec::new();
    if !keep_superseded {
        for name in document.server_names() {
            let Some(entry) = document.server(&name) else {
                continue;
            };
            let Some(core) = entry.superseded_core() else {
                continue;
            };
            if document.remove_server(&name) {
                migrated.push(format!("{name} (superseded {core})"));
            }
        }
    }
    document.set_profile_server(SERVER_NAME, profile);
    let new_content = match document.marshal() {
        Ok(content) => content,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain install: encode {}: {error}",
                location.path.display()
            );
            return exit::GENERIC;
        }
    };

    if dry_run {
        let diff = symbrain_harness::unified_diff_bytes(
            &symbrain_core::go_path::os_bytes(location.path.as_os_str()),
            &original,
            &new_content,
        );
        if diff.is_empty() {
            let _ = writeln!(
                stdout,
                "{}: already up to date, no changes to make",
                location.path.display()
            );
        } else {
            let _ = stdout.write_all(&diff);
            if !migrated.is_empty() {
                let _ = writeln!(
                    stdout,
                    "migrated superseded core entries: {}",
                    migrated.join(", ")
                );
            }
        }
        return exit::OK;
    }

    let capability = match capability {
        Some(capability) => capability,
        None => match AtomicFile::open(
            &location.trusted_root,
            &location.relative_path,
            location.path.clone(),
            true,
        ) {
            Ok(capability) => capability,
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "symbrain install: create {}: {error}",
                    location.path.display()
                );
                return exit::GENERIC;
            }
        },
    };
    if let (Some(snapshot), true) = (snapshot.as_ref(), existed) {
        match capability.backup_snapshot(snapshot, &Utc::now().format("%Y%m%dT%H%M%SZ").to_string())
        {
            Ok(backup) => {
                let _ = writeln!(
                    stdout,
                    "backed up {} to {}",
                    location.path.display(),
                    backup.display()
                );
                if !migrated.is_empty() {
                    let _ = writeln!(
                        stdout,
                        "migrated superseded core entries: {} (roll back by restoring {})",
                        migrated.join(", "),
                        backup.display()
                    );
                }
            }
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "symbrain install: back up {}: {error}",
                    location.path.display()
                );
                return exit::GENERIC;
            }
        }
    }

    if let Err(error) = capability.write(&new_content, 0o600) {
        let _ = writeln!(
            stderr,
            "symbrain install: write {}: {error}",
            location.path.display()
        );
        return exit::GENERIC;
    }
    let mut line = b"installed symbrain into ".to_vec();
    line.extend_from_slice(&symbrain_core::go_path::os_bytes(location.path.as_os_str()));
    line.extend_from_slice(format!(" (harness: {}, profile: ", harness.name).as_bytes());
    line.extend_from_slice(profile.as_ref());
    line.extend_from_slice(b")\n");
    let _ = stdout.write_all(&line);
    exit::OK
}

fn open_existing(
    location: &ConfigLocation,
) -> io::Result<(Option<AtomicFile>, Option<symbrain_harness::FileSnapshot>)> {
    let capability = match AtomicFile::open(
        &location.trusted_root,
        &location.relative_path,
        location.path.clone(),
        false,
    ) {
        Ok(capability) => capability,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => return Err(error),
    };
    match capability.read_snapshot() {
        Ok(snapshot) => Ok((Some(capability), Some(snapshot))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok((None, None)),
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_lines)]
fn uninstall_from(
    harness: &Harness,
    location: &ConfigLocation,
    dry_run: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let (capability, snapshot) = match open_existing(location) {
        Ok(value) => value,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain uninstall: read {}: {error}",
                location.path.display()
            );
            return exit::GENERIC;
        }
    };
    let Some(snapshot) = snapshot else {
        let _ = writeln!(
            stdout,
            "{}: no config file found, nothing to uninstall",
            location.path.display()
        );
        return exit::OK;
    };
    let original = snapshot.bytes.clone();
    let mut document = match symbrain_harness::parse(harness, &original) {
        Ok(document) => document,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain uninstall: {}: {}",
                location.path.display(),
                format_parse_error(harness, &original, &error)
            );
            return exit::NO_INPUT;
        }
    };
    let Some(entry) = document.server(SERVER_NAME) else {
        let _ = writeln!(
            stdout,
            "{}: symbrain is not installed, nothing to do",
            location.path.display()
        );
        return exit::OK;
    };
    if !entry.is_symbrain() {
        let _ = writeln!(
            stdout,
            "{}: symbrain is not installed, nothing to do",
            location.path.display()
        );
        return exit::OK;
    }
    document.remove_server(SERVER_NAME);
    let new_content = match document.marshal() {
        Ok(content) => content,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain uninstall: encode {}: {error}",
                location.path.display()
            );
            return exit::GENERIC;
        }
    };
    if dry_run {
        let diff = symbrain_harness::unified_diff_bytes(
            &symbrain_core::go_path::os_bytes(location.path.as_os_str()),
            &original,
            &new_content,
        );
        let _ = stdout.write_all(&diff);
        return exit::OK;
    }
    let Some(capability) = capability else {
        return exit::GENERIC;
    };
    match capability.backup_snapshot(&snapshot, &Utc::now().format("%Y%m%dT%H%M%SZ").to_string()) {
        Ok(backup) => {
            let _ = writeln!(
                stdout,
                "backed up {} to {}",
                location.path.display(),
                backup.display()
            );
        }
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain uninstall: back up {}: {error}",
                location.path.display()
            );
            return exit::GENERIC;
        }
    }
    if let Err(error) = capability.write(&new_content, 0o600) {
        let _ = writeln!(
            stderr,
            "symbrain uninstall: write {}: {error}",
            location.path.display()
        );
        return exit::GENERIC;
    }
    let _ = writeln!(
        stdout,
        "removed symbrain from {} (harness: {})",
        location.path.display(),
        harness.name
    );
    exit::OK
}

fn format_parse_error(harness: &Harness, original: &[u8], error: &HarnessError) -> String {
    let detail = match error {
        HarnessError::Json(message) => go_json_error_detail(original, message),
        HarnessError::Toml(message) => message.clone(),
        _ => error.to_string(),
    };
    let format = harness.format.to_string();
    format!(
        "harness: {} config is not valid {format}; refusing to edit a config symbrain cannot parse: parse {format}: {detail}",
        harness.name
    )
}

fn go_json_error_detail(original: &[u8], message: &str) -> String {
    let prefix = "expected `\"` at byte ";
    if let Some(position) = message
        .strip_prefix(prefix)
        .and_then(|value| value.parse::<usize>().ok())
    {
        return original.get(position).map_or_else(
            || message.to_owned(),
            |byte| format!("invalid character '{}'", *byte as char),
        );
    }
    message.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_all_install_flags() {
        let args = [
            OsString::from("--harness"),
            OsString::from("claude"),
            OsString::from("--profile=p"),
            OsString::from("--project"),
            OsString::from("/tmp/project"),
            OsString::from("--dry-run"),
            OsString::from("--keep-superseded"),
        ];
        let mut stderr = Vec::new();
        let parsed = parse_install_args(&args, &mut stderr).unwrap();
        assert_eq!(parsed.harness, "claude");
        assert_eq!(parsed.profile, "p");
        assert_eq!(parsed.project, "/tmp/project");
        assert!(parsed.dry_run);
        assert!(parsed.keep_superseded);
        assert!(stderr.is_empty());
    }

    #[test]
    fn foreign_symbrain_entry_is_not_removed() {
        let harness = symbrain_harness::lookup("cursor").unwrap();
        let document = symbrain_harness::parse(
            harness,
            br#"{"mcpServers":{"symbrain":{"command":"other-tool","args":["x"]}}}"#,
        )
        .unwrap();
        let entry = document.server(SERVER_NAME).unwrap();
        assert!(!entry.is_symbrain());
        assert_eq!(document.server(SERVER_NAME), Some(entry));
    }

    #[test]
    fn superseded_names_are_exact_basenames() {
        assert_eq!(
            symbrain_harness::SUPERSEDED_CORE_NAMES,
            ["symmemory", "symskills"]
        );
        assert_eq!(Entry::new("p").command, SERVER_NAME);
        assert!(
            Entry {
                command: "/bin/symmemory".into(),
                args: vec![]
            }
            .superseded_core()
            .is_some()
        );
        assert!(
            Entry {
                command: "/bin/symvault".into(),
                args: vec![]
            }
            .superseded_core()
            .is_none()
        );
    }
}
