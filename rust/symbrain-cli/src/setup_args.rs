//! Go flag-set semantics for setup.
use super::parse_go_bool;
use std::ffi::OsString;
use std::io::Write;
use symbrain_core::config::{format_go_quoted_bytes, os_bytes};
use symbrain_core::exit;

const USAGE: &str = "Usage of setup:\n  -allow-unsigned\n    \tinstall even if cosign or a core's signature is unavailable (prints a warning; skips publisher verification for that core)\n  -fix\n    \trepair missing or version-mismatched binaries (alias for doctor --fix)\n  -force-release\n    \twith --fix: allow replacing a brain-source build with the pinned release download\n  -from-source string\n    \tbuild optional module binaries from the in-repo sources at this repository root and install them into the managed directory (instead of downloading releases)\n  -json\n    \temit machine-readable JSON\n  -modules string\n    \twith --from-source: comma-separated module selection (browse,operate,scope); default: modules enabled in config\n";

#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // Independent Go flag-set booleans.
pub(super) struct SetupArgs {
    pub(super) json: bool,
    pub(super) fix: bool,
    pub(super) allow_unsigned: bool,
    pub(super) force_release: bool,
    pub(super) from_source: OsString,
    pub(super) modules: Vec<u8>,
}

pub(super) fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<SetupArgs, u8> {
    let normalized = crate::normalize_flags(args);
    let mut parsed = SetupArgs::default();
    let mut arguments = normalized.into_iter();
    while let Some(raw_argument) = arguments.next() {
        let argument = os_bytes(&raw_argument);
        if argument.as_ref() == b"--" || argument.as_ref() == b"-" || !argument.starts_with(b"-") {
            break;
        }
        let flag = argument.strip_prefix(b"--").unwrap_or(&argument[1..]);
        let (name, value) = flag
            .iter()
            .position(|byte| *byte == b'=')
            .map_or((flag, None), |at| (&flag[..at], Some(&flag[at + 1..])));
        if name.is_empty() || name.starts_with(b"-") || name.starts_with(b"=") {
            let _ = stderr.write_all(b"bad flag syntax: ");
            let _ = stderr.write_all(&argument);
            let _ = write!(stderr, "\n{USAGE}");
            return Err(exit::USAGE);
        }
        if matches!(name, b"from-source" | b"modules") {
            let raw_value = if let Some(value) = value {
                os_value(value)
            } else if let Some(value) = arguments.next() {
                value
            } else {
                let _ = stderr.write_all(b"flag needs an argument: -");
                let _ = stderr.write_all(name);
                let _ = write!(stderr, "\n{USAGE}");
                return Err(exit::USAGE);
            };
            if name == b"from-source" {
                parsed.from_source = raw_value;
            } else {
                parsed.modules = os_bytes(&raw_value).into_owned();
            }
            continue;
        }
        if matches!(name, b"h" | b"help") {
            let _ = write!(stderr, "{USAGE}");
            return Err(exit::USAGE);
        }
        let target = match name {
            b"json" => &mut parsed.json,
            b"fix" => &mut parsed.fix,
            b"allow-unsigned" => &mut parsed.allow_unsigned,
            b"force-release" => &mut parsed.force_release,
            _ => {
                let _ = stderr.write_all(b"flag provided but not defined: -");
                let _ = stderr.write_all(name);
                let _ = write!(stderr, "\n{USAGE}");
                return Err(exit::USAGE);
            }
        };
        let boolean = value.map_or(Some(true), |bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| parse_go_bool(text).ok())
        });
        if let Some(value) = boolean {
            *target = value;
        } else {
            let quoted = format_go_quoted_bytes(value.expect("invalid explicit boolean"));
            let _ = write!(stderr, "invalid boolean value {quoted} for -");
            let _ = stderr.write_all(name);
            let _ = write!(stderr, ": parse error\n{USAGE}");
            return Err(exit::USAGE);
        }
    }
    Ok(parsed)
}

fn os_value(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes.to_vec())
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(bytes).into_owned().into()
    }
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
    fn help_lists_the_retained_module_lifecycle_flags() {
        let mut stderr = Vec::new();
        let result = parse_args(&["--help".into()], &mut stderr);

        assert_eq!(result.unwrap_err(), exit::USAGE);
        assert_eq!(stderr, USAGE.as_bytes());
    }
}
