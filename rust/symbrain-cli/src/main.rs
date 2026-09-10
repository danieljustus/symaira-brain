#![deny(unsafe_code)]

use std::ffi::OsString;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    ExitCode::from(symbrain_cli::run(&args, &mut stdout, &mut stderr))
}
