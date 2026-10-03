//! Human vault administration; secret values only travel over child stdin.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use symbrain_core::exit;

use crate::doctor_cli::doctor_process::run_process_with_input;
use crate::passthrough;

#[path = "vault_response.rs"]
mod response;

pub(super) fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let action = args.get(1).and_then(|arg| arg.to_str());
    let rest = args.get(2..).unwrap_or_default();
    match action {
        Some("create") => create_or_set("create", rest, stdout, stderr),
        Some("set") => create_or_set("set", rest, stdout, stderr),
        Some("delete") => delete(rest, stdout, stderr),
        _ => passthrough::run(args, stderr),
    }
}

fn usage(action: &str, stderr: &mut dyn Write) -> u8 {
    let suffix = match action {
        "create" => "create <path> < secret.txt",
        "set" => "set <path.field> < secret.txt",
        _ => "delete <path> --yes",
    };
    let _ = writeln!(
        stderr,
        "symbrain vault {action}: usage: symbrain vault {suffix}"
    );
    exit::USAGE
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.chars().any(|ch| ch < '\u{20}')
        && !path.split('/').any(|part| part.is_empty() || part == "..")
}

fn field_target(query: &str) -> Option<(&str, &str)> {
    let (path, field) = query.rsplit_once('.')?;
    (valid_path(path) && !field.is_empty() && !field.chars().any(|ch| ch < '\u{20}'))
        .then_some((path, field))
}

fn read_secret(mut stdin: impl Read) -> Result<Vec<u8>, String> {
    let mut value = Vec::new();
    stdin
        .read_to_end(&mut value)
        .map_err(|error| format!("read secret from stdin: {error}"))?;
    if value.ends_with(b"\r\n") {
        value.truncate(value.len() - 2);
    } else if value.ends_with(b"\n") {
        value.pop();
    }
    if value.contains(&b'\n') || value.contains(&b'\r') {
        return Err("multiline secret values are not supported".to_owned());
    }
    // An invalid UTF-8 byte is not whitespace in Go's bytes.TrimSpace either.
    if std::str::from_utf8(&value).is_ok_and(|text| text.trim().is_empty()) {
        return Err("secret value from stdin is empty".to_owned());
    }
    Ok(value)
}

fn command(
    binary: &Path,
    args: &[OsString],
    input: Option<Vec<u8>>,
    timeout: u64,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    let (status, stdout, _) =
        run_process_with_input(binary, args, input, Duration::from_secs(timeout))?;
    Ok((status, stdout))
}

#[derive(Default)]
struct Metadata {
    path: Option<String>,
    fields: Option<BTreeMap<String, Value>>,
}

struct FieldValue(Value);
impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        symbrain_audit::decode_go_json_value(raw.get().as_bytes())
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("invalid vault metadata field value"))
    }
}

impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MetadataVisitor;
        impl<'de> Visitor<'de> for MetadataVisitor {
            type Value = Metadata;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("vault metadata object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Metadata, E> {
                Ok(Metadata::default())
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Metadata, M::Error> {
                let mut result = Metadata::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.replace('\u{017f}', "s").to_ascii_lowercase().as_str() {
                        // Go ignores null for string fields and merges repeated
                        // map fields; preserve order rather than parsing Value first.
                        "path" => {
                            if let Some(path) = map.next_value::<Option<String>>()? {
                                result.path = Some(path);
                            }
                        }
                        "fields" => {
                            match map.next_value::<Option<BTreeMap<String, FieldValue>>>()? {
                                Some(fields) => result
                                    .fields
                                    .get_or_insert_with(BTreeMap::new)
                                    .extend(fields.into_iter().map(|(key, value)| (key, value.0))),
                                None => result.fields = None,
                            }
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(result)
            }
        }
        deserializer.deserialize_any(MetadataVisitor)
    }
}

fn metadata(bytes: &[u8]) -> Result<Metadata, serde_json::Error> {
    serde_json::from_slice(bytes)
}

fn read_metadata(binary: &Path, path: &OsStr) -> Result<Vec<u8>, String> {
    let args = [
        OsString::from("get"),
        path.to_os_string(),
        OsString::from("--output"),
        OsString::from("json"),
    ];
    let (status, out) = command(binary, &args, None, 60)?;
    if status.success() {
        Ok(out)
    } else {
        Err("confirmation failed".to_owned())
    }
}

fn error(action: &str, message: &str, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "symbrain vault {action}: {message}");
    exit::GENERIC
}

fn create_or_set(
    action: &str,
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let [query] = args else {
        return usage(action, stderr);
    };
    let display = query.to_string_lossy();
    let (_, field) = if action == "set" {
        let Some((path, field)) = field_target(&display) else {
            return usage(action, stderr);
        };
        (path, Some(field))
    } else {
        if !valid_path(&display) {
            return usage(action, stderr);
        }
        (display.as_ref(), None)
    };
    let value = match read_secret(std::io::stdin().lock()) {
        Ok(value) => value,
        Err(message) => {
            let _ = writeln!(stderr, "symbrain vault {action}: {message}");
            return exit::USAGE;
        }
    };
    let Some(binary) = passthrough::admin_binary(action, stderr) else {
        return exit::GENERIC;
    };
    let mut input = value.clone();
    input.push(b'\n');
    let child_args = [
        OsString::from(if action == "create" { "add" } else { "set" }),
        query.clone(),
        OsString::from("--stdin-value"),
    ];
    if !command(&binary, &child_args, Some(input), 60).is_ok_and(|(status, _)| status.success()) {
        return error(action, &format!("{action} failed"), stderr);
    }
    let raw_path = if field.is_some() {
        field_path(query)
    } else {
        query.clone()
    };
    let Ok(bytes) = read_metadata(&binary, &raw_path) else {
        return error(action, "confirmation read failed", stderr);
    };
    let Ok(detail) = metadata(&bytes) else {
        return error(
            action,
            if action == "create" {
                "confirmation was not valid JSON"
            } else {
                "confirmation read failed"
            },
            stderr,
        );
    };
    let fields = detail.fields.unwrap_or_default();
    let confirmed_path = detail.path.as_deref().filter(|path| !path.is_empty());
    if let Some(field) = field {
        let raw_field_valid = std::str::from_utf8(
            query
                .as_encoded_bytes()
                .rsplit(|byte| *byte == b'.')
                .next()
                .unwrap_or_default(),
        )
        .is_ok();
        if !raw_field_valid
            || fields
                .get(field)
                .and_then(Value::as_str)
                .is_none_or(|actual| actual.as_bytes() != value)
        {
            return error(
                action,
                "updated field confirmation did not match requested value",
                stderr,
            );
        }
    }
    response::write(
        stdout,
        &raw_path,
        confirmed_path,
        field,
        Some(fields.len()),
        false,
    )
}

fn field_path(query: &OsStr) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = query.as_bytes();
        let dot = bytes
            .iter()
            .rposition(|byte| *byte == b'.')
            .expect("validated field target");
        OsString::from_vec(bytes[..dot].to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(
            query
                .to_string_lossy()
                .rsplit_once('.')
                .expect("validated field target")
                .0,
        )
    }
}

fn delete(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let [path, yes] = args else {
        return usage("delete", stderr);
    };
    if yes != "--yes" && yes != "-y" {
        return usage("delete", stderr);
    }
    let Some(binary) = passthrough::admin_binary("delete", stderr) else {
        return exit::GENERIC;
    };
    let args = [
        OsString::from("delete"),
        path.clone(),
        OsString::from("--yes"),
    ];
    if !command(&binary, &args, None, 60).is_ok_and(|(status, _)| status.success()) {
        return error("delete", "delete failed", stderr);
    }
    let args = [
        OsString::from("get"),
        path.clone(),
        OsString::from("--output"),
        OsString::from("json"),
    ];
    let code = command(&binary, &args, None, 30)
        .ok()
        .and_then(|(status, _)| status.code())
        .unwrap_or(-1);
    match code {
        2 => response::write(stdout, path, None, None, None, true),
        0 => error(
            "delete",
            "confirmation read found entry still present",
            stderr,
        ),
        _ => error(
            "delete",
            &format!(
                "deletion submitted but absence verification failed (symvault get exit {code})"
            ),
            stderr,
        ),
    }
}
