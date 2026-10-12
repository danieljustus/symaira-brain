//! Command discovery is not flag admission: Cobra searches before pflag parses.
//! Source contract: dcddcef0 Browse, Cobra 1.10.2 command.go stripFlags/Find.

use super::{Format, ParseError, parse_bool, parse_format};

/// Find the next command word without interpreting string values as commands.
/// Discovery uses the current group's local and inherited NoOptDefVal flags;
/// Cobra installs the default help/version flags later, in execute().
pub(super) fn command_index(values: &[String], start: usize, group: &str) -> Option<usize> {
    let mut index = start;
    while let Some(value) = values.get(index) {
        if value == "--" {
            return None;
        }
        if value == "--json"
            || (group == "daemon" && matches!(value.as_str(),
                "--ssrf" | "--allow-private" | "--headless" | "--mcp-mode"))
        {
            index += 1;
        } else if !value.contains('=')
            && (value.starts_with("--") || (value.starts_with('-') && value.len() == 2))
        {
            index += 2;
        } else if !value.is_empty() && !value.starts_with('-') {
            return Some(index);
        } else {
            index += 1;
        }
    }
    None
}

pub(super) fn child_index(values: &[String], command: usize) -> Option<usize> {
    let group = values[command].as_str();
    command_index(values, command + 1, group).filter(|&i| match group {
        "daemon" => matches!(values[i].as_str(), "status" | "stop"),
        "session" => matches!(values[i].as_str(), "list" | "info" | "id"),
        "state" => matches!(values[i].as_str(), "save" | "load" | "list" | "show" | "clear" | "clean" | "key"),
        _ => true,
    })
}

/// Effective-parser tokens, excluding string flag values and everything after
/// the delimiter. Help/version spellings used as values are never switches.
pub(super) fn token_indices(values: &[String]) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut index = 0;
    while let Some(value) = values.get(index) {
        if value == "--" {
            break;
        }
        indices.push(index);
        let boolean = matches!(
            value.as_str(),
            "--json" | "-h" | "--help" | "-v" | "--version" | "--bail" | "--dry-run"
                | "--check" | "--no-descriptions" | "--list-profiles" | "--allow-private"
                | "--ssrf" | "--headless" | "--mcp-mode" | "--exact" | "--compact" | "--interactive"
                | "--urls" | "--diff" | "--engine-hint"
        );
        index += if value.len() > 1 && value.starts_with('-')
            && !value.contains('=') && !boolean && !is_help_flag(value)
        {
            2
        } else {
            1
        };
    }
    indices
}

pub(super) fn is_help_flag(value: &str) -> bool {
    value == "--help" || value.starts_with("--help=")
        || value.strip_prefix('-').is_some_and(|v| {
            let name = v.split('=').next().unwrap_or(v);
            !name.is_empty() && name.bytes().all(|b| b == b'h')
        })
}

fn help_value(value: &str) -> Result<bool, ParseError> {
    if !value.starts_with("--") && value.ends_with('=') {
        return Err(flag_error("unknown shorthand flag: '=' in -="));
    }
    parse_bool("-h, --help", value.split_once('=').map_or("true", |(_, v)| v))
        .map_err(|error| flag_error(&error.message))
}

pub(super) fn help_requested(values: &[String]) -> Result<bool, ParseError> {
    let mut help = false;
    for i in token_indices(values) {
        if is_help_flag(&values[i]) {
            help = help_value(&values[i])?;
        }
    }
    Ok(help)
}

/// Admit session flags only on a command that actually declares/inherits one.
/// Parsers still consume every value in original order (including raw bytes).
pub(super) fn validate(
    values: &[String],
    command: Option<usize>,
    error_format: &mut Format,
) -> Result<(), ParseError> {
    let indices = token_indices(values);
    let name = command.map(|i| values[i].as_str()).unwrap_or("");
    let subcommand = command.and_then(|i| child_index(values, i));
    let verb = subcommand.map_or("", |i| values[i].as_str());
    let inspection = matches!((name, verb),
        ("daemon", "status" | "stop") | ("session", "list" | "info")
        | ("state", "list" | "show"));
    let group = matches!(name, "daemon" | "session" | "state") && subcommand.is_none();
    // Native DaemonProxy startup already uses --mode; Go inspection/help does not.
    // Do not propagate a later help error before the first flag is validated.
    let native_start = name == "daemon" && group && !help_requested(values).unwrap_or(true);
    if !inspection && !group && !indices.iter().any(|&i| {
        values[i] == "--session" || values[i].starts_with("--session=")
    }) {
        return Ok(());
    }
    let supported = match name {
        "daemon" | "session" | "state" | "mcp" | "open" | "goto" | "back" | "forward"
        | "reload" | "wait" | "snapshot" | "read" | "click" | "fill" | "type" | "press"
        | "get" | "is" | "find" | "a11y" => true,
        "flow" | "workflow" => subcommand.is_some_and(|i| values[i] == "run"),
        // The Rust-only fetch surface keeps its existing local flag contract.
        "fetch" => true,
        _ => false,
    };
    // Unknown commands are diagnosed by command discovery, before flag parsing.
    if !supported && !matches!(name, "" | "version" | "upgrade" | "config" | "batch"
        | "profiles" | "tools" | "completion" | "help" | "flow" | "workflow")
    {
        return Ok(());
    }
    let mut output = "text";
    let mut json = false;
    let result = (|| {
        for i in indices {
            let value = values[i].as_str();
            if is_help_flag(value) {
                help_value(value)?;
                continue;
            }
            // These final command surfaces have only string/bool flags. Check
            // the complete flag grammar before help, not a session-only subset.
            // Other session-capable commands retain their owning value parsers.
            if inspection || group || !supported {
                if let Some(flag) = value.strip_prefix("--") {
                    if flag.starts_with('-') || flag.starts_with('=') {
                        return Err(flag_error(&format!("bad flag syntax: {value}")));
                    }
                    let flag = flag.split('=').next().unwrap_or(flag);
                    let boolean = match flag {
                        "json" => true,
                        "session" if supported => false,
                        "output" => false,
                        "restore" | "profile" if name == "daemon" => false,
                        "ssrf" | "allow-private" | "headless" | "mcp-mode"
                            if name == "daemon" && group => true,
                        "allowed-domains" | "engine" | "cdp-endpoint"
                            if name == "daemon" && group => false,
                        "mode" if native_start => false,
                        "check" if name == "upgrade" => true,
                        "bail" | "dry-run" if name == "batch" => true,
                        "no-descriptions" if name == "completion" => true,
                        "version" if command.is_none() => true,
                        "log-level" | "log-format" | "config-dir" | "cache-dir"
                        | "state-dir" | "executable-path" if name == "config" && verb == "show" => false,
                        "profiles" | "tools" if name == "tools" => false,
                        _ => return Err(flag_error(&format!("unknown flag: --{flag}"))),
                    };
                    if boolean {
                        // JSON assignment (including failure) is handled below.
                        if flag != "json" && let Some((_, raw)) = value.split_once('=') {
                            parse_bool(&format!("--{flag}"), raw)
                                .map_err(|error| flag_error(&error.message))?;
                        }
                    } else if !value.contains('=') && i + 1 == values.len() {
                        return Err(flag_error(&format!("flag needs an argument: --{flag}")));
                    }
                } else if let Some(mut short) = value.strip_prefix('-').filter(|s| !s.is_empty()) {
                    while let Some(first) = short.bytes().next() {
                        let label = match first {
                            b'h' => "-h, --help",
                            b'v' if command.is_none() => "-v, --version",
                            _ => return Err(flag_error(&format!(
                                "unknown shorthand flag: {:?} in -{short}", char::from(first)
                            ))),
                        };
                        short = &short[1..];
                        if short.len() > 1 && short.starts_with('=') {
                            parse_bool(label, &short[1..])
                                .map_err(|error| flag_error(&error.message))?;
                            break;
                        }
                    }
                }
            }
            match value {
                "--json" => json = true,
                "--output" => {
                    output = values.get(i + 1).map(String::as_str)
                        .ok_or_else(|| flag_error("flag needs an argument: --output"))?;
                }
                value if value.starts_with("--json=") => {
                    // pflag boolValue.Set assigns ParseBool's false result even
                    // on error; a failed repeat must clear an earlier true.
                    json = false;
                    json = parse_bool("--json", &value[7..])
                        .map_err(|error| flag_error(&error.message))?;
                }
                value if value.starts_with("--output=") => output = &value[9..],
                _ => {}
            }
            if value == "--session" || value.starts_with("--session=") {
                if !supported {
                    return Err(flag_error("unknown flag: --session"));
                }
                if value == "--session" && i + 1 == values.len() {
                    return Err(flag_error("flag needs an argument: --session"));
                }
            }
            // A child does not inherit the root's local --version flag.
            if command.is_some()
                && (matches!(value, "-v" | "--version") || value.starts_with("--version="))
            {
                return Err(flag_error(if value == "-v" {
                    "unknown shorthand flag: 'v' in -v"
                } else {
                    "unknown flag: --version"
                }));
            }
        }
        Ok(())
    })();
    // Flag errors see only the consumed prefix; later positional errors see the
    // fully parsed flags. An invalid output spelling falls back to text in Go.
    *error_format = if json {
        Format::Json
    } else {
        parse_format(output).unwrap_or(Format::Text)
    };
    result
}

fn flag_error(message: &str) -> ParseError {
    // normalizeCommandError does not classify pflag's colon-form unknown flag
    // or missing-value errors: they retain Go's generic exit status.
    ParseError {
        message: message.to_owned(),
        exit_code: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Action, Format, parse, parse_with_output};
    use std::ffi::OsString;

    fn argv(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn selected(action: Action) -> (OsString, Format) {
        match action {
            Action::DaemonLifecycle { session, format, .. }
            | Action::StateOperation { session, format, .. } => (session, format),
            Action::Dispatch { session, format, .. }
            | Action::FlowRun { session, format, .. }
            | Action::DaemonRun { session, format, .. } => (session.into(), format),
            Action::Mcp { session, .. } => (session.into(), Format::Text),
            other => panic!("expected selected session, got {other:?}"),
        }
    }

    #[test]
    fn session_prefixes_preserve_selection_and_command_local_admission() {
        for (group, verb) in [
            ("daemon", "status"),
            ("session", "list"),
            ("session", "info"),
            ("state", "list"),
        ] {
            for position in 0..3 {
                for inline in [false, true] {
                    let mut values = vec![group, verb];
                    let flag = if inline {
                        vec!["--session=alpha"]
                    } else {
                        vec!["--session", "alpha"]
                    };
                    drop(values.splice(position..position, flag));
                    values.extend(["--json", "--output=yaml"]);
                    let action = parse(&argv(&values)).unwrap();
                    match &action {
                        Action::DaemonLifecycle { command, .. }
                        | Action::StateOperation { command, .. } => {
                            assert_eq!(command, &format!("{group}.{verb}"));
                        }
                        other => panic!("wrong dispatch: {other:?}"),
                    }
                    assert_eq!(selected(action), ("alpha".into(), Format::Json));
                }
            }
            for (prefix, suffix, expected, format) in [
                (vec!["--session=first"], vec!["--session", "last"], "last", Format::Text),
                (vec!["--session", "--help"], vec![], "--help", Format::Text),
                (vec!["--session", "--version"], vec![], "--version", Format::Text),
                (vec!["--session", "--"], vec![], "--", Format::Text),
                (vec!["--session="], vec![], "", Format::Text),
                (vec!["--session", "state"], vec!["--"], "state", Format::Text),
                (vec!["--json", "--session=alpha"], vec!["--json=false", "--output=yaml"], "alpha", Format::Yaml),
                (vec!["--output=invalid", "--session=alpha"], vec!["--json"], "alpha", Format::Json),
            ] {
                let mut values = prefix;
                values.extend([group, verb]);
                values.extend(suffix);
                assert_eq!(selected(parse(&argv(&values)).unwrap()), (expected.into(), format));
            }
            let error = parse(&argv(&["--session=alpha", group, verb, "--session"])).unwrap_err();
            assert_eq!(error.message, "flag needs an argument: --session");
            assert_eq!(error.exit_code, 1);
            let error = parse(&argv(&["--session=alpha", group, verb, "--bogus=x"])).unwrap_err();
            assert_eq!(error.message, "unknown flag: --bogus");
            assert_eq!(error.exit_code, 1);
            assert!(parse(&argv(&["--session=alpha", group, verb, "--", "--session=omega"])).is_err());
            let prefixed = parse(&argv(&["--session=alpha", group, "--session", "beta", verb, "--help"]));
            assert_eq!(prefixed, parse(&argv(&[group, verb, "--help"])));
        }

        for (values, expected) in [
            (vec!["--json", "--session=alpha", "version"], Format::Json),
            (vec!["--session=alpha", "version", "--json"], Format::Text),
            (vec!["--session=alpha", "daemon", "status", "--json", "--session"], Format::Json),
            (vec!["--output=yaml", "--session=alpha", "daemon", "status", "--bogus=x"], Format::Yaml),
            (vec!["--session=alpha", "daemon", "status", "--bogus=x", "--json"], Format::Text),
        ] {
            let mut format = Format::Text;
            assert!(parse_with_output(&argv(&values), &mut format).is_err());
            assert_eq!(format, expected, "{values:?}");
        }

        for values in [
            vec!["--session=alpha", "version"],
            vec!["--session", "alpha", "version", "--help"],
            vec!["--session=alpha", "profiles"],
            vec!["--session=alpha", "config", "show"],
            vec!["--session=alpha", "batch", "--dry-run", "version"],
            vec!["--session=alpha", "flow", "list"],
            vec!["flow", "--session", "alpha", "validate", "file.yaml"],
            vec!["--session=alpha", "help", "session", "list"],
            vec!["--session=alpha"],
            vec!["--session=alpha", "--", "session", "list"],
        ] {
            let error = parse(&argv(&values)).unwrap_err();
            assert_eq!(error.message, "unknown flag: --session", "{values:?}");
            assert_eq!(error.exit_code, 1);
        }

        // Only parser actions are inspected: never run browser/MCP/flow/key code.
        for values in [
            vec!["--session=alpha", "open", "https://fixture.invalid", "--json"],
            vec!["--json", "--session", "alpha", "goto", "https://fixture.invalid"],
            vec!["--session=alpha", "back", "--json", "--output=yaml"],
            vec!["--session=alpha", "flow", "run", "file.yaml", "--json"],
            vec!["flow", "--session=alpha", "run", "file.yaml", "--json"],
            vec!["--session=first", "workflow", "run", "file.yaml", "--session=alpha", "--json"],
        ] {
            assert_eq!(selected(parse(&argv(&values)).unwrap()), ("alpha".into(), Format::Json));
        }
        assert_eq!(
            selected(parse(&argv(&["--output=yaml", "--session=alpha", "mcp"])).unwrap()),
            ("alpha".into(), Format::Text)
        );
        assert_eq!(
            parse(&argv(&["--session=alpha", "state", "--session", "key", "key", "init", "--json"])),
            Ok(Action::StateKeyInit { format: Format::Json })
        );

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            for (group, verb) in [("daemon", "status"), ("session", "info"), ("state", "list")] {
                for inline in [false, true] {
                    let bytes = b"bad\xffsession".to_vec();
                    let mut raw = if inline {
                        let mut flag = b"--session=".to_vec();
                        flag.extend_from_slice(&bytes);
                        vec![OsString::from_vec(flag)]
                    } else {
                        vec!["--session".into(), OsString::from_vec(bytes.clone())]
                    };
                    raw.extend(argv(&[group, verb, "--json"]));
                    assert_eq!(selected(parse(&raw).unwrap()), (OsString::from_vec(bytes), Format::Json));
                }
            }
        }
    }
}
