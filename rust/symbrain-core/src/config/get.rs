//! Native implementation of `symbrain config get`.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value};

use super::format::{self, TomlRef};

fn lookup_key<'a>(doc: &'a DocumentMut, key: &str) -> Option<TomlRef<'a>> {
    let mut current = TomlRef::Table(doc.as_table());
    for part in key.split('.') {
        current = match current {
            TomlRef::Table(table) => match table.get(part)? {
                Item::Table(table) => TomlRef::Table(table),
                Item::Value(Value::InlineTable(table)) => TomlRef::InlineTable(table),
                Item::Value(value) => TomlRef::Value(value),
                Item::ArrayOfTables(array) => TomlRef::ArrayOfTables(array),
                Item::None => return None,
            },
            TomlRef::InlineTable(table) => match table.get(part)? {
                Value::InlineTable(table) => TomlRef::InlineTable(table),
                value => TomlRef::Value(value),
            },
            _ => return None,
        };
    }
    Some(current)
}

/// Executes `symbrain config get` at the resolved global configuration path.
pub fn run_config_get(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let path = crate::xdg::config_path();
    run_config_get_with_path(&path, args, stdout, stderr)
}

/// Executes `symbrain config get` using an explicit configuration file path.
pub fn run_config_get_with_path(
    path: &Path,
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if args.is_empty() {
        return match fs::read(path) {
            Ok(bytes) => stdout
                .write_all(&bytes)
                .map_or(crate::exit::GENERIC, |()| crate::exit::OK),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let _ = writeln!(stdout, "(no config file at {})", path.display());
                crate::exit::OK
            }
            Err(err) => {
                let _ = writeln!(
                    stderr,
                    "symbrain config get: read {}: {err}",
                    path.display()
                );
                crate::exit::GENERIC
            }
        };
    }
    if args.len() > 1 {
        let _ = writeln!(
            stderr,
            "symbrain config get: unexpected argument {}",
            format::format_go_quoted(&args[1])
        );
        return crate::exit::USAGE;
    }

    let quoted_key = format::format_go_quoted(&args[0]);
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let _ = writeln!(
                stderr,
                "symbrain config get: key {quoted_key} is not set in {}",
                path.display()
            );
            return crate::exit::USAGE;
        }
        Err(err) => {
            let _ = writeln!(
                stderr,
                "symbrain config get: parse {}: {err}",
                path.display()
            );
            return crate::exit::GENERIC;
        }
    };
    let doc: DocumentMut = match content.parse() {
        Ok(doc) => doc,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "symbrain config get: parse {}: {err}",
                path.display()
            );
            return crate::exit::GENERIC;
        }
    };
    if let Some(node) = args[0].to_str().and_then(|key| lookup_key(&doc, key)) {
        let mut rendered = String::new();
        format::format_toml_ref(node, &mut rendered);
        return writeln!(stdout, "{rendered}").map_or(crate::exit::GENERIC, |()| crate::exit::OK);
    }
    let _ = writeln!(
        stderr,
        "symbrain config get: key {quoted_key} is not set in {}",
        path.display()
    );
    crate::exit::USAGE
}

#[cfg(test)]
#[path = "get_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bytes_tests.rs"]
mod bytes_tests;
