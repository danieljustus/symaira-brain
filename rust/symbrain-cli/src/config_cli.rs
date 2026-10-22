use std::ffi::OsString;
use std::io::Write;

use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_core::xdg;

use crate::go_flags::FlagSet;

const CONFIG_USAGE: &str = r"symbrain config — inspect and edit the global config file

Usage:
  symbrain config path                 Print the global config file path
  symbrain config get [key]            Print the stored value for a dotted key
                                       (no key: print the whole file)
  symbrain config set <key> <value>    Set a dotted key; values are typed as
                                       bool, integer, or string
  symbrain config set --preview ...    Show the change without writing
                                       (writes <config>.bak before mutation)
  symbrain config set --no-backup ...  Explicitly opt out of the backup

Keys live under ~/.config/symbrain/config.toml, e.g. default_profile,
audit.enabled, audit.verbose, gateway.identity_injection,
updatecheck.enabled, patterns.enabled, patterns.promotion_threshold,
servers.vault.binary_path, servers.operate.binary_path,
servers.scope.binary_path, modules.browse, modules.operate, modules.scope.

modules.* selects optional capability modules (default: all disabled).
Enabling one only changes what 'symbrain setup'/'symbrain doctor --fix'
install; it does not change any profile's tool exposure on its own.

Note: get/set operate on the global file as stored. The config loader also
merges a project-local .symbrain.toml and SYMBRAIN_* environment overrides
on top when commands run. Creating the file is 'symbrain init'; profiles
have their own commands ('symbrain profile ...').
";

/// Runs `symbrain config`, mirroring Go's `cmdConfig`: an argument-less
/// `flag.FlagSet` parse (so `-h` and unknown flags print the usage), then the
/// first positional selects the subcommand.
pub(crate) fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let normalized = super::normalize_flags(args);
    let flags = FlagSet {
        string_flags: &[],
        usage: CONFIG_USAGE,
    };
    let Ok(parsed) = flags.parse(&normalized, stderr) else {
        return exit::NO_INPUT;
    };
    let Some((subcommand, sub_args)) = parsed.rest.split_first() else {
        let _ = write!(stderr, "{CONFIG_USAGE}");
        return exit::NO_INPUT;
    };

    match subcommand.to_str() {
        Some("path") => run_path(sub_args, stdout),
        Some("get") => symbrain_core::config::run_config_get(sub_args, stdout, stderr),
        Some("set") => symbrain_core::config::run_config_set(sub_args, stdout, stderr),
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain config: unknown subcommand {} (want path, get, or set)",
                format_go_quoted(subcommand)
            );
            exit::NO_INPUT
        }
    }
}

fn run_path(args: &[OsString], stdout: &mut dyn Write) -> u8 {
    if let Some(unexpected) = args.first() {
        let _ = writeln!(
            stdout,
            "symbrain config path: unexpected argument {:?}",
            unexpected.to_string_lossy()
        );
        return exit::USAGE;
    }
    let path = xdg::config_path();
    if writeln!(stdout, "{}", path.display()).is_ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute(args: &[&str]) -> (u8, String, String) {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&args, &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).expect("stdout is UTF-8"),
            String::from_utf8(stderr).expect("stderr is UTF-8"),
        )
    }

    #[test]
    fn no_subcommand_matches_go_usage() {
        let (code, stdout, stderr) = execute(&[]);
        assert_eq!(code, exit::NO_INPUT);
        assert!(stdout.is_empty());
        assert_eq!(stderr, CONFIG_USAGE);
    }

    #[test]
    fn unknown_subcommand_matches_go_error() {
        let (code, stdout, stderr) = execute(&["frobnicate"]);
        assert_eq!(code, exit::NO_INPUT);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            "symbrain config: unknown subcommand \"frobnicate\" (want path, get, or set)\n"
        );
    }
}
