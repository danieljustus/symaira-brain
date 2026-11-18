//! `symbrain browse|operate|scope`: pass-through to the built-in workers.
//!
//! The workers ship in the Brain release bundle as separate processes
//! (`symbrowse`, and on macOS `symoperate` and `symscope`). The command hands
//! every remaining argument to the worker unchanged, with inherited stdio and
//! the worker's exit code, after resolving it with the broker's lookup order
//! (next to the running executable, `~/.symaira/bin`, then `PATH`).

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;

use crate::passthrough;

/// A built-in worker reachable through a `symbrain` subcommand.
struct Worker {
    command: &'static str,
    binary: &'static str,
    macos_only: bool,
}

const WORKERS: [Worker; 3] = [
    Worker {
        command: "browse",
        binary: "symbrowse",
        macos_only: false,
    },
    Worker {
        command: "operate",
        binary: "symoperate",
        macos_only: true,
    },
    Worker {
        command: "scope",
        binary: "symscope",
        macos_only: true,
    },
];

/// Runs the worker behind `command` with `args` (everything after the command).
pub(super) fn run(command: &str, args: &[OsString], stderr: &mut dyn Write) -> u8 {
    run_on(command, args, cfg!(target_os = "macos"), stderr)
}

fn run_on(command: &str, args: &[OsString], macos: bool, stderr: &mut dyn Write) -> u8 {
    let Some(worker) = WORKERS.iter().find(|worker| worker.command == command) else {
        let _ = writeln!(stderr, "symbrain: unknown worker command {command:?}");
        return exit::USAGE;
    };
    if worker.macos_only && !macos {
        let _ = writeln!(
            stderr,
            "symbrain {command}: {} is unavailable on this platform (macOS only)",
            worker.binary
        );
        return exit::GENERIC;
    }
    if let Ok(path) = symbrain_broker::discover(worker.binary, "") {
        return passthrough::execute_child(
            std::path::Path::new(&path),
            args,
            &format!("symbrain {command}"),
            stderr,
        );
    }
    let _ = writeln!(
        stderr,
        "symbrain {command}: {} not found; it ships with symbrain since the bundled release, so reinstall or upgrade symbrain",
        worker.binary
    );
    exit::GENERIC
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_capturing(command: &str, macos: bool) -> (u8, String) {
        let mut stderr = Vec::new();
        let code = run_on(command, &[], macos, &mut stderr);
        (code, String::from_utf8(stderr).expect("utf-8 stderr"))
    }

    #[test]
    fn macos_only_workers_are_unavailable_elsewhere_before_any_lookup() {
        for command in ["operate", "scope"] {
            let (code, stderr) = run_capturing(command, false);
            assert_eq!(code, exit::GENERIC, "{command}");
            assert_eq!(stderr.lines().count(), 1, "{stderr}");
            assert!(stderr.contains("unavailable on this platform"), "{stderr}");
        }
    }

    #[test]
    fn only_operate_and_scope_are_macos_only() {
        let restricted: Vec<_> = WORKERS
            .iter()
            .filter(|worker| worker.macos_only)
            .map(|worker| worker.command)
            .collect();
        assert_eq!(restricted, ["operate", "scope"]);
    }

    #[test]
    fn unknown_command_is_a_usage_error() {
        let (code, _) = run_capturing("bogus", true);
        assert_eq!(code, exit::USAGE);
    }
}
