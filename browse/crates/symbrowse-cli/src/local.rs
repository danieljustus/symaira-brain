//! Commands that do not (only) forward a daemon frame.

use std::process::ExitCode;

use symbrowse_core::{
    batch::{self, ItemOutput},
    config::FlagOverrides,
    output::Format,
};
use symbrowse_protocol::{render_version_json, render_version_text};

use crate::{
    VERSION,
    app::{CliError, Ctx, Outcome},
    cobra, legacy,
};

fn format_of(ctx: &Ctx<'_>) -> Result<Format, CliError> {
    ctx.format()
}

pub fn dispatch(ctx: &mut Ctx<'_>, key: &str, args: &[String]) -> Outcome {
    let result: Result<Outcome, CliError> = (|| {
        Ok(match key {
            "version" => {
                if ctx.structured() {
                    let text = render_version_json(VERSION).map_err(|e| CliError::plain(e.to_string()))?;
                    ctx.print(&text);
                } else {
                    ctx.print(&render_version_text(VERSION));
                }
                Outcome::Done
            }
            "help" => {
                help(ctx, args);
                Outcome::Done
            }
            "completion bash" | "completion zsh" | "completion fish" | "completion powershell" => {
                let shell = ctx.path[1].clone();
                let script = if ctx.flags.bool("no-descriptions") {
                    crate::completion::script_without_descriptions(&shell)
                } else {
                    crate::completion::script(&shell).map(str::to_owned)
                };
                match script {
                    Some(script) => ctx.print(&script),
                    None => return Err(CliError::plain(format!("unsupported shell {shell:?}"))),
                }
                Outcome::Done
            }
            "config show" => {
                let format = format_of(ctx)?;
                let flag = |name: &str| ctx.flags.changed(name).then(|| ctx.flags.string(name, ""));
                let overrides = FlagOverrides {
                    log_level: flag("log-level"),
                    log_format: flag("log-format"),
                    config_dir: flag("config-dir"),
                    cache_dir: flag("cache-dir"),
                    state_dir: flag("state-dir"),
                    executable_path: flag("executable-path"),
                    engine: None,
                    mode: None,
                };
                Outcome::Raw(legacy::run_config_show(format, overrides))
            }
            "mcp" => {
                if ctx.flags.bool("list-profiles") {
                    ctx.print(legacy::MCP_PROFILE_LIST);
                    Outcome::Done
                } else {
                    let engine = ctx.flags.changed("engine").then(|| ctx.flags.string("engine", ""));
                    Outcome::Raw(legacy::run_mcp(
                        ctx.flags.string("session", "default"),
                        ctx.flags.string("tools", "core"),
                        ctx.flags.bool("allow-private"),
                        engine,
                    ))
                }
            }
            "daemon" => Outcome::Raw(daemon(ctx)?),
            "state key init" => Outcome::Raw(legacy::run_state_key_init(format_of(ctx)?)),
            "flow list" => Outcome::Raw(legacy::run_flow_list(format_of(ctx)?)),
            "flow validate" => Outcome::Raw(legacy::run_flow_validate(args[0].clone().into(), format_of(ctx)?)),
            "profiles" => Outcome::Raw(crate::browser_profiles::run(format_of(ctx)?)),
            "upgrade" => Outcome::Raw(crate::upgrade::run(format_of(ctx)?, VERSION)),
            "batch" => batch_command(ctx, args)?,
            other => Outcome::Fail(CliError::plain(format!(
                "symbrowse {other} is not implemented natively in the Rust CLI yet"
            ))),
        })
    })();
    result.unwrap_or_else(Outcome::Fail)
}

fn daemon(ctx: &Ctx<'_>) -> Result<ExitCode, CliError> {
    let tri = |name: &str| ctx.flags.changed(name).then(|| ctx.flags.bool(name));
    Ok(legacy::run_daemon(
        ctx.flags.string("session", "default"),
        String::new(),
        ctx.flags.string("engine", "chrome"),
        tri("ssrf"),
        tri("allow-private"),
        ctx.format()?,
    ))
}

fn help(ctx: &mut Ctx<'_>, args: &[String]) {
    let tree = crate::app::tree();
    let found = cobra::find(tree, args);
    if found.legacy_error.is_some() {
        let quoted: Vec<String> = args.iter().map(|a| backquote(a)).collect();
        ctx.print(&format!("Unknown help topic [{}]\n", quoted.join(" ")));
        ctx.print(&tree.usage(0));
    } else {
        ctx.print(&tree.nodes[found.index].help);
    }
}

/// `%#q`: raw string when possible, Go-quoted otherwise.
fn backquote(value: &str) -> String {
    if value.contains('`') || value.chars().any(|c| c.is_control() && c != '\t') {
        symbrowse_core::go_quote::quote(value)
    } else {
        format!("`{value}`")
    }
}

fn batch_command(ctx: &mut Ctx<'_>, args: &[String]) -> Result<Outcome, CliError> {
    let mut commands: Vec<String> = args.to_vec();
    if commands.is_empty() {
        let mut raw = Vec::new();
        ctx.stdin
            .read_to_end(&mut raw)
            .map_err(|e| CliError::plain(format!("read batch commands from stdin: {e}")))?;
        let input = String::from_utf8_lossy(&raw).into_owned();
        let unexpected_end = input.trim().is_empty();
        let prefix = "stdin must contain a JSON array of command strings: ";
        commands = if unexpected_end {
            return Err(CliError::plain(format!("{prefix}unexpected end of JSON input")));
        } else {
            decode_batch_commands(&input).map_err(|e| CliError::plain(format!("{prefix}{e}")))?
        };
    }
    if commands.is_empty() {
        return Err(CliError::invalid_args("batch requires at least one command"));
    }
    let report = batch::run(
        &commands,
        ctx.flags.bool("dry-run"),
        ctx.flags.bool("bail"),
        |argv: &[String]| -> ItemOutput {
            let (stdout, error) = crate::app::run_item(argv, VERSION);
            ItemOutput {
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                error: error.map(|e| e.error_string()),
            }
        },
    );
    let format = ctx.format()?;
    let text = match format {
        Format::Json => {
            let mut out = serde_json::to_string(&legacy::BatchSuccess { success: true, data: &report })
                .map_err(|e| CliError::plain(e.to_string()))?;
            out.push('\n');
            out
        }
        Format::Text => {
            let mut out = serde_json::to_string_pretty(&report).map_err(|e| CliError::plain(e.to_string()))?;
            out = html_escape(&out);
            out.push('\n');
            out
        }
        Format::Yaml => batch::render_yaml(&report),
    };
    ctx.print(&text);
    Ok(Outcome::Done)
}

fn html_escape(text: &str) -> String {
    text.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn decode_batch_commands(input: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(input).map_err(|error| {
        if error.is_eof() {
            return "unexpected end of JSON input".to_owned();
        }
        let trimmed = input.trim_start();
        if trimmed.starts_with('n') && trimmed != "null" {
            let expected = "null";
            if let Some((index, actual)) = trimmed
                .chars()
                .enumerate()
                .find(|(index, character)| expected.chars().nth(*index) != Some(*character))
            {
                let wanted = expected.chars().nth(index).unwrap_or(' ');
                return format!(
                    "invalid character {actual:?} in literal null (expecting {wanted:?})"
                );
            }
        }
        error.to_string()
    })?;
    let serde_json::Value::Array(items) = value else {
        if value.is_null() {
            return Ok(Vec::new());
        }
        return Err(format!(
            "json: cannot unmarshal {} into Go value of type []string",
            json_type_name(&value)
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            serde_json::Value::String(command) => Ok(command),
            serde_json::Value::Null => Ok(String::new()),
            other => Err(format!(
                "json: cannot unmarshal {} into Go value of type string",
                json_type_name(&other)
            )),
        })
        .collect()
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

