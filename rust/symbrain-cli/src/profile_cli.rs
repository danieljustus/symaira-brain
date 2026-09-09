use std::ffi::OsString;
use std::io::{self, Write};

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_policy::policy;
use symbrain_policy::profile::{self, Profile, ServerConfig};

use crate::profile_actions;
use crate::profile_args::{normalize_flags, parse_list_args, parse_show_args, quote_os};
use crate::profile_render as render;

#[derive(Debug, Serialize)]
pub(crate) struct ProfileListEntry {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) servers: Vec<ServerSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerSummary {
    pub(crate) server: String,
    pub(crate) enabled: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) mode: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileShowReport {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) audit: symbrain_policy::AuditConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
    pub(crate) servers: Vec<ServerShowReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerShowReport {
    pub(crate) server: String,
    pub(crate) enabled: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools_allow: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools_deny: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) command: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effective_policy: Option<policy::Report>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) note: String,
}

/// Handles the migrated profile subcommands without invoking the Go fallback.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    let Some(subcommand) = args.first().map(OsString::as_os_str) else {
        let _ = render::print_usage(stderr);
        return Some(exit::USAGE);
    };
    match subcommand.to_string_lossy().as_ref() {
        "list" => Some(run_list(&args[1..], stdout, stderr, format)),
        "show" => Some(run_show(&args[1..], stdout, stderr, format)),
        "add" => Some(profile_actions::run_add(&args[1..], stdout, stderr)),
        "remove" => Some(profile_actions::run_remove(&args[1..], stdout, stderr)),
        "help" | "--help" | "-h" => {
            let result = render::print_usage(stdout);
            Some(write_result(&result, stderr))
        }
        unknown => {
            let _ = writeln!(stderr, "symbrain profile: unknown subcommand {unknown:?}\n");
            let _ = render::print_usage(stderr);
            Some(exit::USAGE)
        }
    }
}

fn run_list(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let Ok(positionals) = parse_list_args(args, stderr) else {
        return exit::USAGE;
    };
    if let Some(unexpected) = positionals.first() {
        let _ = writeln!(
            stderr,
            "symbrain profile list: unexpected argument {}",
            quote_os(unexpected)
        );
        return exit::USAGE;
    }

    let results = match profile::load_all() {
        Ok(results) => results,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain profile list: {error}");
            return error.exit_code();
        }
    };
    let entries = results
        .into_iter()
        .map(|result| match (result.profile, result.err) {
            (Some(profile), None) => ProfileListEntry {
                name: profile.name.clone(),
                description: profile.description.clone(),
                servers: server_summaries(&profile),
                error: None,
            },
            (_, Some(error)) => ProfileListEntry {
                name: result.name,
                description: String::new(),
                servers: Vec::new(),
                error: Some(error.to_string()),
            },
            _ => unreachable!("load result must have either a profile or an error"),
        })
        .collect::<Vec<_>>();

    let rendered = output::render(stdout, format, &entries, |writer| {
        render::print_list(writer, &entries)
    });
    if rendered.is_err() {
        let _ = writeln!(stderr, "symbrain profile list: format output");
        exit::GENERIC
    } else {
        exit::OK
    }
}

fn run_show(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let args = normalize_flags(args);
    let Ok(positionals) = parse_show_args(&args, stderr) else {
        return exit::USAGE;
    };
    if positionals.len() != 1 {
        let _ = writeln!(stderr, "usage: symbrain profile show <name> [--json]");
        return exit::USAGE;
    }
    let name = positionals[0].to_string_lossy();
    let profile = match profile::load(&name) {
        Ok(profile) => profile,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain profile show: {}",
                format_profile_error(&error)
            );
            return error.exit_code();
        }
    };
    let report = build_show_report(&profile);
    let rendered = output::render(stdout, format, &report, |writer| {
        render::print_show(writer, &report)
    });
    if rendered.is_err() {
        let _ = writeln!(stderr, "symbrain profile show: format output");
        exit::GENERIC
    } else {
        exit::OK
    }
}

fn build_show_report(profile: &Profile) -> ProfileShowReport {
    ProfileShowReport {
        name: profile.name.clone(),
        description: profile.description.clone(),
        audit: profile.audit,
        warnings: profile.warnings.clone(),
        servers: profile
            .server_aliases()
            .into_iter()
            .map(|alias| {
                let config = profile.server(&alias);
                build_server_report(&alias, &config)
            })
            .collect(),
    }
}

fn build_server_report(alias: &str, config: &ServerConfig) -> ServerShowReport {
    let mut report = ServerShowReport {
        server: alias.to_string(),
        enabled: config.enabled,
        mode: config.mode.clone(),
        tools_allow: config.tools_allow.clone(),
        tools_deny: config.tools_deny.clone(),
        command: config.command.clone(),
        args: config.args.clone(),
        url: config.url.clone(),
        effective_policy: None,
        note: String::new(),
    };
    if symbrain_policy::is_core_alias(alias)
        && (alias == symbrain_policy::SERVER_VAULT || alias == symbrain_policy::SERVER_MEMORY)
    {
        match policy::evaluate_preset(alias, config) {
            Ok(effective) => report.effective_policy = Some(effective),
            Err(error) => report.note = error.to_string(),
        }
    } else if alias == symbrain_policy::SERVER_SKILLS {
        report.note = "skills has no mode preset; effective tools are always-full-when-enabled, narrowed only by tools_allow/tools_deny, and require a live connection to enumerate".to_string();
    } else if alias == symbrain_policy::SERVER_USAGE {
        report.note = "usage has no mode preset; the single tool get_ai_usage is exposed when enabled, narrowed only by tools_allow/tools_deny".to_string();
    } else {
        report.note =
            "foreign server: no mode preset; exposure is read/write classified per profile"
                .to_string();
    }
    report
}

fn server_summaries(profile: &Profile) -> Vec<ServerSummary> {
    profile
        .server_aliases()
        .into_iter()
        .map(|alias| {
            let server = profile.server(&alias);
            ServerSummary {
                server: alias,
                enabled: server.enabled,
                mode: server.mode,
            }
        })
        .collect()
}

fn format_profile_error(error: &symbrain_policy::ProfileError) -> String {
    match error {
        symbrain_policy::ProfileError::InvalidName { name, message } => {
            format!("profile: invalid name {name:?}: {message}")
        }
        other => other.to_string(),
    }
}

fn write_result(result: &io::Result<()>, stderr: &mut dyn Write) -> u8 {
    if result.is_ok() {
        exit::OK
    } else {
        let _ = writeln!(stderr, "symbrain profile: format output");
        exit::GENERIC
    }
}
