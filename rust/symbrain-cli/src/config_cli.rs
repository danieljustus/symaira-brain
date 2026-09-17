use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::xdg;

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

pub(crate) fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Option<u8> {
    let normalized = super::normalize_flags(args);
    let (subcommand, sub_args) = if normalized.first().is_some_and(|a| a == "--") {
        let sub = normalized.get(1).map(|s| s.to_string_lossy());
        let sub_args = if normalized.len() > 2 {
            &normalized[2..]
        } else {
            &[]
        };
        (sub, sub_args)
    } else {
        let sub = normalized.first().map(|s| s.to_string_lossy());
        let sub_args = if normalized.len() > 1 {
            &normalized[1..]
        } else {
            &[]
        };
        (sub, sub_args)
    };

    match subcommand.as_deref() {
        Some("path") => Some(run_path(sub_args, stdout)),
        Some("get") => Some(symbrain_core::config::run_config_get(
            sub_args, stdout, stderr,
        )),
        Some("set") => Some(symbrain_core::config::run_config_set(
            sub_args, stdout, stderr,
        )),
        Some(sub) if !sub.starts_with('-') => {
            let _ = writeln!(
                stderr,
                "symbrain config: unknown subcommand {sub:?} (want path, get, or set)"
            );
            Some(exit::NO_INPUT)
        }
        None => {
            let _ = write!(stderr, "{CONFIG_USAGE}");
            Some(exit::NO_INPUT)
        }
        _ => None,
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

    fn execute(args: &[&str]) -> (Option<u8>, String, String) {
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
        assert_eq!(code, Some(exit::NO_INPUT));
        assert!(stdout.is_empty());
        assert_eq!(stderr, CONFIG_USAGE);
    }

    #[test]
    fn unknown_subcommand_matches_go_error() {
        let (code, stdout, stderr) = execute(&["frobnicate"]);
        assert_eq!(code, Some(exit::NO_INPUT));
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            "symbrain config: unknown subcommand \"frobnicate\" (want path, get, or set)\n"
        );
    }
}
