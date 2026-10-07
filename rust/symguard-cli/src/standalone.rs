//! Standalone presentation over the same Guard-domain command handlers.
use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;

const USAGE: &str = "symguard — local-first security gateway for AI agents\n\nUsage:\n  symguard <command> [flags]\n\nCommands:\n  version   Print version and build info\n  doctor    Check system health and configuration\n  decide    Read a JSON decision request from stdin, write the decision to stdout\n  grants    List and revoke standing grants\n  scan      Discover MCP servers across supported AI clients\n  help      Show this help message\n\nRun 'symguard <command> --help' for details on a specific command.\n";

/// Runs the standalone Guard without invoking Brain or a legacy executable.
/// Unsupported doctor states fail closed until their native diagnostics port.
pub fn run_standalone(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let Some(verb) = args.first() else {
        let _ = stdout.write_all(USAGE.as_bytes());
        return exit::USAGE;
    };
    match verb.to_str() {
        Some("help" | "--help" | "-h") => {
            let _ = stdout.write_all(USAGE.as_bytes());
            exit::OK
        }
        Some("version" | "doctor" | "decide" | "grants" | "scan") => {
            crate::run(args, stdout, stderr).unwrap_or_else(|| {
                let _ = writeln!(stderr, "symguard doctor: unsupported native diagnostic state; no legacy fallback is available");
                exit::GENERIC
            })
        }
        _ => {
            let quote = symbrain_core::config::format_go_quoted(verb);
            let _ = writeln!(stderr, "symguard: unknown command {quote}\n");
            let _ = stderr.write_all(USAGE.as_bytes());
            exit::USAGE
        }
    }
}
