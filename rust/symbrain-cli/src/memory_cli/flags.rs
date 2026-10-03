//! Raw argument handling matching Go flag sets and positional reordering.

use super::{flag_integer, flag_usage};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use symbrain_core::config::format_go_quoted;

#[derive(Default)]
pub(super) struct Args {
    values: BTreeMap<&'static str, Vec<u8>>,
    pub positional: Vec<OsString>,
    pub limit: i64,
    pub staged: bool,
}

impl Args {
    pub fn text(&self, name: &str) -> String {
        self.values
            .get(name)
            .map_or_else(String::new, |value| go_string(value))
    }
    pub fn raw(&self, name: &str) -> OsString {
        self.values
            .get(name)
            .map_or_else(OsString::new, |value| os(value.clone()))
    }
}

pub(super) fn has_help(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "-h" || arg == "--help")
}

pub(super) fn parse(args: &[OsString], verb: &str, stderr: &mut dyn Write) -> Result<Args, ()> {
    let reordered = if matches!(verb, "search" | "set" | "delete") {
        reorder(args, verb)
    } else {
        args.to_vec()
    };
    let normalized = crate::normalize_flags(&reordered);
    let mut parsed = Args::default();
    if verb == "set" {
        parsed.values.insert("scope", b"global".to_vec());
        parsed.values.insert("author", b"cli:symbrain".to_vec());
    }
    let mut index = 0;
    while let Some(argument) = normalized.get(index) {
        let argument = bytes(argument);
        if argument == b"--" {
            index += 1;
            break;
        }
        if !argument.starts_with(b"-") || argument == b"-" {
            break;
        }
        let flag = argument.strip_prefix(b"--").unwrap_or(&argument[1..]);
        let (name, inline) = flag
            .iter()
            .position(|&b| b == b'=')
            .map_or((flag, None), |n| (&flag[..n], Some(&flag[n + 1..])));
        if name.is_empty() || name.starts_with(b"-") || name.starts_with(b"=") {
            let _ = stderr.write_all(b"bad flag syntax: ");
            let _ = stderr.write_all(&argument);
            let _ = stderr.write_all(b"\n");
            flag_usage::write(verb, stderr);
            return Err(());
        }
        let Some((canonical, kind)) = definition(verb, name) else {
            if !matches!(name, b"h" | b"help") {
                let _ = stderr.write_all(b"flag provided but not defined: -");
                let _ = stderr.write_all(name);
                let _ = stderr.write_all(b"\n");
            }
            flag_usage::write(verb, stderr);
            return Err(());
        };
        let value = if kind == Kind::Boolean && inline.is_none() {
            Some(b"true".to_vec())
        } else {
            inline.map(<[u8]>::to_vec).or_else(|| {
                index += 1;
                normalized.get(index).map(|arg| bytes(arg))
            })
        };
        let Some(value) = value else {
            let _ = stderr.write_all(b"flag needs an argument: -");
            let _ = stderr.write_all(name);
            let _ = stderr.write_all(b"\n");
            flag_usage::write(verb, stderr);
            return Err(());
        };
        let error = match kind {
            Kind::String => {
                parsed.values.insert(canonical, value.clone());
                None
            }
            Kind::Integer => match flag_integer::parse_integer(&value) {
                Ok(number) => {
                    parsed.limit = number;
                    None
                }
                Err(error) => Some(error),
            },
            Kind::Boolean => match parse_boolean(&value) {
                Some(value) => {
                    parsed.staged = value;
                    None
                }
                None => Some("parse error"),
            },
        };
        if let Some(error) = error {
            let _ = write!(
                stderr,
                "invalid {}value {} for {}-",
                if kind == Kind::Boolean {
                    "boolean "
                } else {
                    ""
                },
                format_go_quoted(&os(value)),
                if kind == Kind::Boolean { "" } else { "flag " }
            );
            let _ = stderr.write_all(name);
            let _ = writeln!(stderr, ": {error}");
            flag_usage::write(verb, stderr);
            return Err(());
        }
        index += 1;
    }
    parsed.positional = normalized[index..].to_vec();
    Ok(parsed)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Integer,
    Boolean,
}
fn definition(verb: &str, name: &[u8]) -> Option<(&'static str, Kind)> {
    let canonical = match name {
        b"db" => "db",
        b"scope" | b"s" if matches!(verb, "list" | "rules" | "search" | "set") => "scope",
        b"limit" | b"l" if matches!(verb, "list" | "query-log" | "search") => {
            return Some(("limit", Kind::Integer));
        }
        b"actor" if verb == "query-log" => "actor",
        b"kind" | b"k" if verb == "set" => "kind",
        b"author" if verb == "set" => "author",
        b"metadata" if verb == "set" => "metadata",
        b"entities" if verb == "set" => "entities",
        b"staged" if verb == "set" => return Some(("staged", Kind::Boolean)),
        _ => return None,
    };
    Some((canonical, Kind::String))
}
fn parse_boolean(value: &[u8]) -> Option<bool> {
    match value {
        b"1" | b"t" | b"T" | b"TRUE" | b"true" | b"True" => Some(true),
        b"0" | b"f" | b"F" | b"FALSE" | b"false" | b"False" => Some(false),
        _ => None,
    }
}
fn reorder(args: &[OsString], verb: &str) -> Vec<OsString> {
    let mut flags = Vec::new();
    let mut positionals = Vec::new();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        let raw = bytes(argument);
        if !raw.starts_with(b"-") || raw == b"-" || raw == b"--" {
            positionals.push(argument.clone());
        } else {
            flags.push(argument.clone());
            let name = raw.strip_prefix(b"--").unwrap_or(&raw);
            let name = name.strip_prefix(b"-").unwrap_or(name);
            if definition(verb, name).is_some_and(|(_, kind)| kind != Kind::Boolean)
                && index + 1 < args.len()
            {
                index += 1;
                flags.push(args[index].clone());
            }
        }
        index += 1;
    }
    flags.extend(positionals);
    flags
}

#[cfg(unix)]
pub(super) fn bytes(arg: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}
#[cfg(not(unix))]
pub(super) fn bytes(arg: &OsStr) -> Vec<u8> {
    arg.to_string_lossy().as_bytes().to_vec()
}
#[cfg(unix)]
fn os(value: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(value)
}
#[cfg(not(unix))]
fn os(value: Vec<u8>) -> OsString {
    String::from_utf8_lossy(&value).into_owned().into()
}
pub(super) fn go_string(mut value: &[u8]) -> String {
    let mut text = String::new();
    while !value.is_empty() {
        match std::str::from_utf8(value) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(std::str::from_utf8(&value[..valid]).expect("valid prefix"));
                text.push('\u{fffd}');
                value = &value[valid + 1..];
            }
        }
    }
    text
}
