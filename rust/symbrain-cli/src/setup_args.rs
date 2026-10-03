//! Go flag-set semantics for setup.
use super::parse_go_bool;
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::exit;

const USAGE: &str = "Usage of setup:\n  -allow-unsigned\n    \tinstall even if cosign or a core's signature is unavailable (prints a warning; skips publisher verification for that core)\n  -fix\n    \trepair missing or version-mismatched binaries (alias for doctor --fix)\n  -force-release\n    \twith --fix: allow replacing a brain-source build with the pinned release download\n  -from-source string\n    \tbuild optional module binaries from the in-repo sources at this repository root and install them into the managed directory (instead of downloading releases)\n  -json\n    \temit machine-readable JSON\n  -modules string\n    \twith --from-source: comma-separated module selection (browse,operate,scope); default: modules enabled in config\n";

#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // Independent Go flag-set booleans.
pub(super) struct SetupArgs {
    pub(super) json: bool,
    pub(super) fix: bool,
    pub(super) allow_unsigned: bool,
    pub(super) force_release: bool,
}

pub(super) fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<SetupArgs, u8> {
    let normalized = crate::normalize_flags(args);
    let mut parsed = SetupArgs::default();
    for argument in normalized {
        let argument = argument.to_string_lossy();
        if argument == "--" || argument == "-" || !argument.starts_with('-') {
            break;
        }
        let flag = argument.trim_start_matches('-');
        if flag == "h" || flag == "help" {
            let _ = write!(stderr, "{USAGE}");
            return Err(exit::USAGE);
        }
        let (name, value) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        let target = match name {
            "json" => &mut parsed.json,
            "fix" => &mut parsed.fix,
            "allow-unsigned" => &mut parsed.allow_unsigned,
            "force-release" => &mut parsed.force_release,
            _ => {
                let _ = writeln!(stderr, "flag provided but not defined: -{name}");
                let _ = write!(stderr, "{USAGE}");
                return Err(exit::USAGE);
            }
        };
        if let Ok(parsed_value) = value.map_or(Ok(true), parse_go_bool) {
            *target = parsed_value;
        } else {
            let value = value.expect("invalid explicit boolean");
            let _ = writeln!(
                stderr,
                "invalid boolean value {value:?} for -{name}: parse error"
            );
            let _ = write!(stderr, "{USAGE}");
            return Err(exit::USAGE);
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_and_stop_at_first_positional_match_go_flagset() {
        let mut stderr = Vec::new();
        let parsed = parse_args(
            &[
                "--fix".into(),
                "--allow-unsigned=false".into(),
                "positional".into(),
                "--json".into(),
            ],
            &mut stderr,
        )
        .unwrap();
        assert!(parsed.fix);
        assert!(!parsed.allow_unsigned);
        assert!(!parsed.json);
        assert!(stderr.is_empty());
    }

    #[test]
    fn unknown_flag_has_go_usage_shape() {
        let mut stderr = Vec::new();
        let result = parse_args(&["--unknown".into()], &mut stderr);
        assert_eq!(result.unwrap_err(), exit::USAGE);
        let text = String::from_utf8(stderr).unwrap();
        assert!(text.starts_with("flag provided but not defined: -unknown\nUsage of setup:\n"));
    }

    #[test]
    fn help_lists_the_go_owned_module_lifecycle_flags() {
        let mut stderr = Vec::new();
        let result = parse_args(&["--help".into()], &mut stderr);

        assert_eq!(result.unwrap_err(), exit::USAGE);
        assert_eq!(stderr, USAGE.as_bytes());
    }
}
