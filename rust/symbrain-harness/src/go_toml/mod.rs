//! Go-parity TOML decoding for harness configs.
//!
//! Go reads `codex` `config.toml` with `toml.Unmarshal` from
//! `github.com/BurntSushi/toml` v1.6.0. That library accepts a slightly
//! different language from the `toml_edit` crate and words its errors in its
//! own way, so the lexer and parser are ported line for line; the resulting
//! tree and error text are what `harness list` reports.

mod lexer;
mod parser;
mod time;

pub(crate) use parser::{Table, Value};

use crate::server_info::ValueKind;
use crate::{ServerInfo, go_quote::quote_str};

/// Decodes TOML bytes like `toml.Unmarshal(data, &map[string]any{})`.
///
/// # Errors
/// Returns the exact error text Go reports (`toml: line N: ...`), without
/// the `parse toml: ` wrapper added by `decodeTOMLObject`.
pub(crate) fn decode_object(data: &[u8]) -> Result<Table, String> {
    let mut data = data;
    if data.starts_with(&[0xFF, 0xFE]) || data.starts_with(&[0xFE, 0xFF]) {
        data = &data[2..];
    } else if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        data = &data[3..];
    }
    let probe = &data[..data.len().min(6)];
    if probe.contains(&0) {
        return Err(
            "toml: line 1: files cannot contain NULL bytes; probably using UTF-16; TOML files must be UTF-8"
                .to_owned(),
        );
    }
    match parser::Parser::new(data).parse() {
        Ok(table) => Ok(table),
        Err(parser::Failure::Parse {
            message,
            line,
            last_key,
        }) => {
            if last_key.is_empty() {
                Err(format!("toml: line {line}: {message}"))
            } else {
                Err(format!(
                    "toml: line {line} (last key {}): {message}",
                    quote_str(&last_key)
                ))
            }
        }
        // Go panics with a non-ParseError value; there is no message to match.
        Err(parser::Failure::Bug(message)) => Err(format!("BUG: {message}")),
    }
}

/// Server summaries from a decoded TOML tree (`genericDocument.ServerInfo`).
pub(crate) fn server_infos(root: &Table, servers_key: &str) -> Vec<ServerInfo> {
    let Some(Value::Table(servers)) = root.get(servers_key) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(name, value)| match value {
            Value::Table(table) => Some(server_info(name, table)),
            _ => None,
        })
        .collect()
}

fn server_info(name: &str, table: &Table) -> ServerInfo {
    let mut fields = Vec::new();
    for key in ["command", "url", "transport", "type"] {
        if let Some(Value::String(value)) = table.get(key) {
            fields.push((key.to_owned(), ValueKind::String(value.clone())));
        }
    }
    if let Some(Value::Array(values)) = table.get("args") {
        let strings = values
            .iter()
            .filter_map(|value| match value {
                Value::String(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        fields.push(("args".to_owned(), ValueKind::Strings(strings)));
    }
    if let Some(Value::Table(env)) = table.get("env") {
        fields.push((
            "env".to_owned(),
            ValueKind::Names(env.keys().cloned().collect()),
        ));
    }
    ServerInfo::from_fields(name, &fields)
}
