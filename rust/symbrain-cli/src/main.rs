#![deny(unsafe_code)]

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if !symaira_cutover::run_for_binary("symbrain", cutover_roots) {
        return ExitCode::from(1);
    }
    ExitCode::from(symbrain_cli::run_stdio(&args))
}

/// State an earlier Go release may have written: the current XDG roots and
/// the pre-consolidation `symmemory`/`symskills` locations still read today.
fn cutover_roots(homes: &symaira_cutover::Homes) -> Vec<symaira_cutover::Root> {
    let root = |label, path| symaira_cutover::Root { label, path };
    vec![
        root("config", homes.config.join("symbrain")),
        root("data", homes.data.join("symbrain")),
        root("legacy-memory-config", homes.config.join("symmemory")),
        root("legacy-memory-data", homes.data.join("symmemory")),
        root("legacy-skills-config", homes.config.join("symskills")),
        root("legacy-skills-data", homes.data.join("symskills")),
    ]
}
