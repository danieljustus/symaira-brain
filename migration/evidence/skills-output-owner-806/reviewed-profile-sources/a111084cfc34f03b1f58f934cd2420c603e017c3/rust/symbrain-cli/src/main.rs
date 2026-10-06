#![deny(unsafe_code)]

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    ExitCode::from(symbrain_cli::run_stdio(&args))
}
