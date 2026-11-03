#![deny(unsafe_code)]

mod app;
mod browser_profiles;
mod cobra;
mod completion;
mod gojson;
mod legacy;
mod local;
mod upgrade;

use std::{ffi::OsString, io::Write, process::ExitCode};

const VERSION: &str = match option_env!("SYMBROWSE_VERSION") {
    Some(version) => version,
    None => "dev",
};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == symbrowse_core::key_sources::STARTUP_PROVIDER_ARGUMENT)
    {
        return ExitCode::from(symbrowse_core::key_sources::run_startup_provider(&args[1..]));
    }
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    if let Some(first) = values.first()
        && matches!(first.as_str(), "__complete" | "__completeNoDesc")
    {
        let output = if first == "__completeNoDesc" {
            completion::complete_no_descriptions(&values[1..])
        } else {
            completion::complete(&values[1..])
        };
        return write_stdout(&output);
    }
    let mut stdin = std::io::stdin();
    let execution = app::run(&values, VERSION, &mut stdin);
    let _ = std::io::stdout().write_all(&execution.stdout);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(&execution.stderr);
    execution
        .raw
        .unwrap_or_else(|| ExitCode::from(execution.exit))
}

fn write_stdout(value: &str) -> ExitCode {
    if std::io::stdout().write_all(value.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
