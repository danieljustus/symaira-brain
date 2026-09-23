//! Browser-profile CLI adapter, distinct from the MCP tool-group catalog.

use std::{path::PathBuf, process::ExitCode};
use symbrowse_core::{
    error::ErrorCode,
    output::{Envelope, Format},
    profiles,
};

use super::{
    ParseError, parse_bool, parse_format, required_value, write_batch_error, write_stdout,
};

pub(super) fn run(arguments: &[String]) -> ExitCode {
    let mut json = false;
    let mut output = "text".to_owned();
    let mut positional = None;
    let parsed = (|| -> Result<(), ParseError> {
        let mut index = 0;
        let mut literal = false;
        while index < arguments.len() {
            let value = &arguments[index];
            if literal {
                positional.get_or_insert(value);
            } else {
                match value.as_str() {
                    "--" => literal = true,
                    "--json" => json = true,
                    "--output" => {
                        index += 1;
                        output = required_value(arguments, index, "--output")?.to_owned();
                    }
                    value if value.starts_with("--json=") => {
                        json = parse_bool("--json", &value[7..])?;
                    }
                    value if value.starts_with("--output=") => output = value[9..].to_owned(),
                    value if value.starts_with("--") => {
                        return Err(ParseError {
                            message: format!("unknown flag: {}", value.split('=').next().unwrap()),
                            exit_code: 1,
                        });
                    }
                    value if value.starts_with('-') && value != "-" => {
                        return Err(ParseError {
                            message: format!(
                                "unknown shorthand flag: '{}' in {value}",
                                value.chars().nth(1).unwrap()
                            ),
                            exit_code: 1,
                        });
                    }
                    _ => {
                        positional.get_or_insert(value);
                    }
                }
            }
            index += 1;
        }
        Ok(())
    })();
    // Cobra parses flags before validating positional arguments. A flag failure
    // uses only the output flags parsed before that failure; --json wins over
    // even an invalid --output value when flag parsing succeeds.
    let format = if json {
        Ok(Format::Json)
    } else {
        parse_format(&output)
    };
    if let Err(error) = parsed {
        return write_batch_error(
            format.unwrap_or(Format::Text),
            ErrorCode::Internal,
            &error.message,
            1,
        );
    }
    if let Some(value) = positional {
        return write_batch_error(
            format.unwrap_or(Format::Text),
            ErrorCode::InvalidArgs,
            &format!("unknown command {value:?} for \"symbrowse profiles\""),
            2,
        );
    }
    let format = match format {
        Ok(format) => format,
        Err(error) => {
            return write_batch_error(Format::Text, ErrorCode::InvalidArgs, &error.message, 2);
        }
    };
    // Go profiles.Discover suppresses directory enumeration errors. Do not
    // load browser preferences, initialize a daemon, or consult credentials.
    let found = browser_root()
        .and_then(|root| profiles::discover(&root).ok())
        .unwrap_or_default();
    if format == Format::Text {
        if found.is_empty() {
            return write_stdout("no Chrome profiles found\n");
        }
        let text: String = found
            .iter()
            .map(|profile| {
                format!(
                    "{}\t{}{}\n",
                    profile.name,
                    profile.path.display(),
                    if profile.is_default { " (default)" } else { "" },
                )
            })
            .collect();
        return write_stdout(&text);
    }
    if format == Format::Yaml {
        return write_stdout(&profiles::render_yaml(&found));
    }
    let data = serde_json::json!({"profiles": (!found.is_empty()).then_some(&found)});
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(text) => write_stdout(&text),
        Err(_) => ExitCode::from(1),
    }
}

fn browser_root() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    if home.is_empty() {
        return None;
    }
    Some(if cfg!(target_os = "macos") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("Google")
            .join("Chrome")
            .join("User Data")
    } else {
        PathBuf::from(home).join(".config").join("google-chrome")
    })
}
