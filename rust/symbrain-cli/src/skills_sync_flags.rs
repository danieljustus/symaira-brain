//! Validate native skills sync flags before any filesystem work.

use std::ffi::{OsStr, OsString};
use std::io::Write;

use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;

const USAGE: &str = "Usage of skills sync:\n  -dry-run\n    \treport the plan without writing\n  -scope string\n    \tinstall scope: user or project (default \"user\")\n  -target string\n    \tlimit to one harness target\n";

pub(super) struct Flags {
    pub(super) dry_run: bool,
    pub(super) target: Option<String>,
    pub(super) scope: String,
}

pub(super) fn parse(args: &[OsString], stderr: &mut dyn Write) -> Result<Flags, u8> {
    let mut flags = Flags {
        dry_run: false,
        target: None,
        scope: "user".to_owned(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" || arg == "-" || !arg.starts_with('-') {
            break;
        }
        // Match normalizeFlags followed by Go's flag parser, including its
        // stopping at the first positional and accepting a flag as a value.
        let normalized = arg.strip_prefix("--").map_or_else(
            || arg.clone(),
            |suffix| std::borrow::Cow::Owned(format!("-{suffix}")),
        );
        if normalized == "--" {
            break;
        }
        let name = normalized
            .strip_prefix("--")
            .or_else(|| normalized.strip_prefix('-'))
            .unwrap_or_default();
        if name.is_empty() || name.starts_with(['-', '=']) {
            return flag_error(stderr, Some(format!("bad flag syntax: {normalized}")));
        }
        let (name, inline) = name
            .split_once('=')
            .map_or((name, None), |(name, value)| (name, Some(value)));
        match name {
            "h" | "help" => return flag_error(stderr, None),
            "dry-run" => {
                flags.dry_run = match inline {
                    None | Some("1" | "t" | "T" | "true" | "TRUE" | "True") => true,
                    Some("0" | "f" | "F" | "false" | "FALSE" | "False") => false,
                    Some(value) => {
                        return flag_error(
                            stderr,
                            Some(format!(
                                "invalid boolean value {} for -dry-run: parse error",
                                format_go_quoted(OsStr::new(value)),
                            )),
                        );
                    }
                };
            }
            "target" | "scope" => {
                let value = if let Some(value) = inline {
                    value.to_owned()
                } else {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return flag_error(
                            stderr,
                            Some(format!("flag needs an argument: -{name}")),
                        );
                    };
                    value.to_string_lossy().into_owned()
                };
                if name == "target" {
                    flags.target = Some(value);
                } else {
                    flags.scope = value;
                }
            }
            _ => {
                return flag_error(
                    stderr,
                    Some(format!("flag provided but not defined: -{name}")),
                );
            }
        }
        index += 1;
    }
    let target = flags.target.as_deref().unwrap_or_default().trim();
    let known = symbrain_skills::default_targets();
    if !target.is_empty() && !known.iter().any(|name| name == target) {
        let _ = writeln!(
            stderr,
            "symbrain skills sync: unknown target {} (known: {})",
            format_go_quoted(OsStr::new(target)),
            known.join(", ")
        );
        return Err(exit::USAGE);
    }
    flags.target = (!target.is_empty()).then(|| target.to_owned());
    flags.scope = match flags.scope.trim() {
        "" | "user" => "user".to_owned(),
        "project" => "project".to_owned(),
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills sync: unknown scope {} (known: user, project)",
                format_go_quoted(OsStr::new(&flags.scope))
            );
            return Err(exit::USAGE);
        }
    };
    Ok(flags)
}

fn flag_error(stderr: &mut dyn Write, error: Option<String>) -> Result<Flags, u8> {
    if let Some(error) = error {
        let _ = writeln!(stderr, "{error}");
    }
    let _ = write!(stderr, "{USAGE}");
    Err(exit::USAGE)
}
