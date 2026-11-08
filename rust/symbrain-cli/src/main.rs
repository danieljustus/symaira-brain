#![deny(unsafe_code)]

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // The one-time backup runs on a worker thread: the command starts after at
    // most `START_GRACE` (so `version`, `help` and the `mcp` handshake never
    // wait for a large state tree) and a backup that is still copying when the
    // command ends is cancelled after `EXIT_GRACE`.
    let mut backup = symaira_cutover::start_for_binary("symbrain", cutover_roots);
    if !backup.settle(symaira_cutover::START_GRACE) {
        return ExitCode::from(1);
    }
    let code = symbrain_cli::run_stdio(&args);
    backup.finish(symaira_cutover::EXIT_GRACE);
    ExitCode::from(code)
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
