//! Native implementation of `symbrain config set`.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::Path;
use toml_edit::DocumentMut;

use super::atomic::{atomic_write, create_dir_all};
use super::encode::{ConfigTable, ConfigValue, encode_document, set_key_in_table};

#[must_use]
pub fn os_bytes(s: &OsStr) -> Cow<'_, [u8]> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Cow::Borrowed(s.as_bytes())
    }
    #[cfg(not(unix))]
    {
        Cow::Owned(s.to_string_lossy().into_owned().into_bytes())
    }
}

fn infer_value(bytes: &[u8]) -> ConfigValue {
    match bytes {
        b"1" | b"t" | b"T" | b"TRUE" | b"true" | b"True" => ConfigValue::Boolean(true),
        b"0" | b"f" | b"F" | b"FALSE" | b"false" | b"False" => ConfigValue::Boolean(false),
        _ => {
            if let Some(integer) = std::str::from_utf8(bytes)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
            {
                return ConfigValue::Integer(integer);
            }
            ConfigValue::String(bytes.to_vec())
        }
    }
}

/// Executes `symbrain config set` at the resolved global configuration path.
pub fn run_config_set(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let path = crate::xdg::config_path();
    run_config_set_with_path(&path, args, stdout, stderr)
}

/// Executes `symbrain config set` using an explicit configuration file path.
pub fn run_config_set_with_path(
    path: &Path,
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if args.len() != 2 {
        let _ = writeln!(stderr, "symbrain config set: want exactly <key> <value>");
        return crate::exit::USAGE;
    }

    let key_bytes = os_bytes(&args[0]);
    if key_bytes.is_empty() {
        let _ = writeln!(stderr, "symbrain config set: key must not be empty");
        return crate::exit::USAGE;
    }

    let val_bytes = os_bytes(&args[1]);
    let value = infer_value(&val_bytes);

    let mut table = match fs::read(path) {
        Ok(raw) => {
            let content = super::lossless::to_lossless_str(&raw);
            let doc: DocumentMut = match content.parse() {
                Ok(doc) => doc,
                Err(err) => {
                    let _ = writeln!(
                        stderr,
                        "symbrain config set: parse {}: {err}",
                        path.display()
                    );
                    return crate::exit::GENERIC;
                }
            };
            ConfigTable::from_document(&doc)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ConfigTable::new(),
        Err(err) => {
            let _ = writeln!(
                stderr,
                "symbrain config set: parse {}: {err}",
                path.display()
            );
            return crate::exit::GENERIC;
        }
    };

    if let Err(err) = set_key_in_table(&mut table, &key_bytes, value) {
        let _ = writeln!(stderr, "symbrain config set: {err}");
        return crate::exit::USAGE;
    }

    let encoded = encode_document(&table);

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if let Err(err) = create_dir_all(parent) {
        let _ = writeln!(
            stderr,
            "symbrain config set: create {}: {err}",
            parent.display()
        );
        return crate::exit::GENERIC;
    }

    if let Err(err) = atomic_write(path, &encoded) {
        let _ = writeln!(
            stderr,
            "symbrain config set: write {}: {err}",
            path.display()
        );
        return crate::exit::GENERIC;
    }

    let _ = stdout.write_all(b"set ");
    let _ = stdout.write_all(&key_bytes);
    let _ = stdout.write_all(b" = ");
    let _ = stdout.write_all(&val_bytes);
    let _ = stdout.write_all(b" in ");
    let _ = stdout.write_all(path.display().to_string().as_bytes());
    let _ = stdout.write_all(b"\n");

    crate::exit::OK
}

#[cfg(test)]
#[path = "set_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "set_fixtures_tests.rs"]
mod fixtures_tests;
