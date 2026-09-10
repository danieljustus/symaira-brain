use std::ffi::OsString;
use std::io::Write;

pub(crate) fn parse_list_args(
    args: &[OsString],
    stderr: &mut dyn Write,
) -> Result<Vec<OsString>, ()> {
    parse_flagless_args(args, "profile list", stderr)
}

pub(crate) fn parse_show_args(
    args: &[OsString],
    stderr: &mut dyn Write,
) -> Result<Vec<OsString>, ()> {
    parse_flagless_args(args, "profile show", stderr)
}

fn parse_flagless_args(
    args: &[OsString],
    usage_name: &str,
    stderr: &mut dyn Write,
) -> Result<Vec<OsString>, ()> {
    let mut positionals = Vec::new();
    let mut parsing_flags = true;
    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
        } else if parsing_flags && is_flag(arg) {
            if matches!(arg.to_string_lossy().as_ref(), "-h" | "-help" | "--help") {
                let _ = writeln!(stderr, "Usage of {usage_name}:");
                return Err(());
            }
            let _ = writeln!(
                stderr,
                "flag provided but not defined: -{}",
                trim_dashes(arg)
            );
            let _ = writeln!(stderr, "Usage of {usage_name}:");
            return Err(());
        } else {
            parsing_flags = false;
            positionals.push(arg.clone());
        }
    }
    Ok(positionals)
}

pub(crate) fn parse_add_args(
    args: &[OsString],
    stderr: &mut dyn Write,
) -> Result<(OsString, OsString), ()> {
    let mut positionals = Vec::new();
    let mut from = OsString::from("restricted");
    let mut i = 0;
    let parsing_flags = true;
    while i < args.len() {
        let arg = &args[i];
        if parsing_flags && arg == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if parsing_flags && let Some(value) = flag_value(arg, "from") {
            from = value;
        } else if parsing_flags && arg == "-from" {
            let Some(value) = args.get(i + 1) else {
                let _ = writeln!(stderr, "flag needs an argument: -from");
                print_add_flag_usage(stderr);
                return Err(());
            };
            from.clone_from(value);
            i += 1;
        } else if parsing_flags && is_flag(arg) {
            if matches!(arg.to_string_lossy().as_ref(), "-h" | "-help" | "--help") {
                print_add_flag_usage(stderr);
                return Err(());
            }
            let _ = writeln!(
                stderr,
                "flag provided but not defined: -{}",
                trim_dashes(arg)
            );
            print_add_flag_usage(stderr);
            return Err(());
        } else {
            positionals.extend(args[i..].iter().cloned());
            break;
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(
            stderr,
            "usage: symbrain profile add <name> [--from personal|restricted]"
        );
        return Err(());
    }
    Ok((positionals.remove(0), from))
}

fn print_add_flag_usage(stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "Usage of profile add:");
    let _ = writeln!(stderr, "  -from string");
    let _ = writeln!(
        stderr,
        "    \ttemplate to create from: \"personal\" or \"restricted\" (default \"restricted\")"
    );
}

#[derive(Debug)]
pub(crate) struct RemoveArgs {
    pub(crate) name: OsString,
    pub(crate) force: bool,
    pub(crate) project: Option<OsString>,
}

pub(crate) fn parse_remove_args(
    args: &[OsString],
    stderr: &mut dyn Write,
) -> Result<RemoveArgs, ()> {
    let mut positionals = Vec::new();
    let mut force = false;
    let mut project = None;
    let mut parsing_flags = true;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if !parsing_flags {
            positionals.push(arg.clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(value) = flag_value(arg, "project") {
            project = Some(value);
        } else if arg == "-project" || arg == "--project" {
            let Some(value) = args.get(i + 1) else {
                print_remove_flag_usage(stderr);
                return Err(());
            };
            project = Some(value.clone());
            i += 1;
        } else if arg == "-force" || arg == "--force" {
            force = true;
        } else if let Some(value) = flag_value(arg, "force") {
            match value.to_string_lossy().as_ref() {
                "1" | "t" | "T" | "true" | "TRUE" | "True" => force = true,
                "0" | "f" | "F" | "false" | "FALSE" | "False" => force = false,
                value => {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {value:?} for -force: parse error"
                    );
                    print_remove_flag_usage(stderr);
                    return Err(());
                }
            }
        } else if is_flag(arg) {
            if matches!(arg.to_string_lossy().as_ref(), "-h" | "-help" | "--help") {
                print_remove_flag_usage(stderr);
                return Err(());
            }
            let _ = writeln!(
                stderr,
                "flag provided but not defined: -{}",
                trim_dashes(arg)
            );
            print_remove_flag_usage(stderr);
            return Err(());
        } else {
            positionals.push(arg.clone());
            parsing_flags = false;
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(
            stderr,
            "usage: symbrain profile remove <name> [--force] [--project <dir>]"
        );
        return Err(());
    }
    Ok(RemoveArgs {
        name: positionals.remove(0),
        force,
        project,
    })
}

fn print_remove_flag_usage(stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "Usage of profile remove:");
    let _ = writeln!(stderr, "  -force");
    let _ = writeln!(stderr, "    \tskip the confirmation prompt");
    let _ = writeln!(stderr, "  -project string");
    let _ = writeln!(
        stderr,
        "    \tproject directory; check project-local harness configs for bindings"
    );
}

fn flag_value(arg: &OsString, flag: &str) -> Option<OsString> {
    let text = arg.to_string_lossy();
    for prefix in [format!("-{flag}="), format!("--{flag}=")] {
        if let Some(value) = text.strip_prefix(&prefix) {
            return Some(OsString::from(value));
        }
    }
    None
}

pub(crate) fn reorder_flags_first(args: &[OsString], value_flags: &[&str]) -> Vec<OsString> {
    let mut flags = Vec::new();
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if is_flag(arg) && arg != "--" {
            flags.push(arg.clone());
            let name = trim_dashes(arg);
            if value_flags.contains(&name.as_str())
                && !arg.to_string_lossy().contains('=')
                && let Some(value) = args.get(i + 1)
            {
                flags.push(value.clone());
                i += 1;
            }
        } else {
            positionals.push(arg.clone());
        }
        i += 1;
    }
    flags.extend(positionals);
    flags
}

pub(crate) fn normalize_flags(args: &[OsString]) -> Vec<OsString> {
    crate::normalize_flags(args)
}

fn is_flag(arg: &OsString) -> bool {
    let text = arg.to_string_lossy();
    text.starts_with('-') && text != "-"
}

fn trim_dashes(arg: &OsString) -> String {
    arg.to_string_lossy().trim_start_matches('-').to_string()
}

#[allow(clippy::unnecessary_debug_formatting)]
pub(crate) fn quote_os(arg: &OsString) -> String {
    format!("{arg:?}")
}
