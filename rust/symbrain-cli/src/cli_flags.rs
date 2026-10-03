//! Raw Go flag normalization and delegated-flag stop boundaries.

use std::ffi::OsString;

/// Normalizes CLI arguments matching Go's `normalizeFlags`:
/// - Converts `--flag` to `-flag` when length > 2
/// - Preserves positional arguments and bare `-`
/// - Preserves `--` and all arguments following `--`
#[must_use]
pub fn normalize_flags(args: &[OsString]) -> Vec<OsString> {
    let mut out = Vec::with_capacity(args.len());
    let mut terminated = false;
    for arg in args {
        if terminated {
            out.push(arg.clone());
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let bytes = arg.as_os_str().as_bytes();
            if bytes == b"--" {
                terminated = true;
                out.push(arg.clone());
            } else if bytes.starts_with(b"--") && bytes.len() > 2 {
                let mut normalized = Vec::with_capacity(bytes.len() - 1);
                normalized.push(b'-');
                normalized.extend_from_slice(&bytes[2..]);
                out.push(OsString::from_vec(normalized));
            } else {
                out.push(arg.clone());
            }
        }
        #[cfg(not(unix))]
        {
            let s = arg.to_string_lossy();
            if s == "--" {
                terminated = true;
                out.push(arg.clone());
            } else if s.starts_with("--") && s.len() > 2 {
                out.push(OsString::from(format!("-{}", &s[2..])));
            } else {
                out.push(arg.clone());
            }
        }
    }
    out
}

/// Returns whether an invocation reaches a Go-owned flag before its native
/// parser would stop. This follows the relevant `flag.FlagSet` boundaries so
/// unrelated invalid invocations do not accidentally require the fallback.
pub(crate) fn has_go_owned_flag(
    args: &[OsString],
    known_flags: &[&str],
    value_flags: &[&str],
    go_owned_flags: &[&str],
    go_owned_bool_flags: &[&str],
) -> bool {
    let normalized = normalize_flags(args);
    let mut index = 0;
    while index < normalized.len() {
        let argument = normalized[index].to_string_lossy();
        if argument == "--" || argument == "-" || !argument.starts_with('-') {
            break;
        }
        let flag = argument.trim_start_matches('-');
        let (name, value) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        if !known_flags.contains(&name) {
            return false;
        }
        if matches!(name, "h" | "help") {
            return false;
        }
        if go_owned_flags.contains(&name)
            || (go_owned_bool_flags.contains(&name) && value != Some("false"))
        {
            return true;
        }
        if value.is_none() && value_flags.contains(&name) {
            index += 1;
            if index == normalized.len() {
                return false;
            }
        }
        index += 1;
    }
    false
}
