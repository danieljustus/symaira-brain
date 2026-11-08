//! Flag grammar of the library-operation verbs (`import`, `pull`, `install`,
//! `render`, `validate`, `history`, `restore`).
//!
//! GNU-style: `--flag value`, `--flag=value`, short aliases, bool flags with an
//! optional `=true|false`, positionals anywhere and `--` ending the flags. The
//! global `--json`/`--output` flags are consumed before dispatch.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;

/// Flags one verb accepts.
pub(super) struct Spec {
    pub verb: &'static str,
    pub bools: &'static [&'static str],
    pub values: &'static [&'static str],
    pub shorts: &'static [(char, &'static str)],
    pub usage: &'static str,
}

/// Parsed arguments.
#[derive(Debug, Default)]
pub(super) struct Args {
    pub positional: Vec<String>,
    set: BTreeSet<String>,
    values: BTreeMap<String, String>,
}

impl Args {
    pub(super) fn flag(&self, name: &str) -> bool {
        self.set.contains(name)
    }

    pub(super) fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

fn usage_error(spec: &Spec, stderr: &mut dyn Write, message: &str) -> u8 {
    let _ = writeln!(stderr, "symbrain skills {}: {message}\n", spec.verb);
    let _ = write!(stderr, "{}", spec.usage);
    exit::USAGE
}

/// Parses `args`; `Err` carries the exit code after help or a diagnostic.
pub(super) fn parse(
    spec: &Spec,
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<Args, u8> {
    let mut parsed = Args::default();
    let mut iter = args.iter();
    while let Some(raw) = iter.next() {
        let Some(arg) = raw.to_str() else {
            return Err(usage_error(spec, stderr, "arguments must be valid UTF-8"));
        };
        if arg == "--" {
            for rest in iter.by_ref() {
                parsed.positional.push(rest.to_string_lossy().into_owned());
            }
            break;
        }
        if arg == "-h" || arg == "--help" {
            let _ = write!(stdout, "{}", spec.usage);
            return Err(exit::OK);
        }
        let Some(body) = arg.strip_prefix("--").or_else(|| {
            arg.strip_prefix('-')
                .filter(|rest| rest.chars().count() == 1)
        }) else {
            parsed.positional.push(arg.to_owned());
            continue;
        };
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        let name = if arg.starts_with("--") {
            name
        } else {
            name.chars()
                .next()
                .and_then(|short| spec.shorts.iter().find(|(c, _)| *c == short))
                .map_or(name, |(_, long)| *long)
        };
        if spec.bools.contains(&name) {
            match inline {
                None | Some("true") => parsed.set.insert(name.to_owned()),
                Some("false") => parsed.set.remove(name),
                Some(other) => {
                    let message = format!("invalid boolean value {other:?} for --{name}");
                    return Err(usage_error(spec, stderr, &message));
                }
            };
        } else if spec.values.contains(&name) {
            let value = if let Some(value) = inline {
                value.to_owned()
            } else if let Some(value) = iter.next().and_then(|value| value.to_str()) {
                value.to_owned()
            } else {
                let message = format!("flag needs an argument: --{name}");
                return Err(usage_error(spec, stderr, &message));
            };
            parsed.values.insert(name.to_owned(), value);
        } else {
            let message = format!("unknown flag: {arg}");
            return Err(usage_error(spec, stderr, &message));
        }
    }
    Ok(parsed)
}
