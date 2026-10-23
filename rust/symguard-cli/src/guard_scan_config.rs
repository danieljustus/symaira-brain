//! Configuration parsing for the native `symbrain guard scan` inventory.

use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Entry {
    pub(crate) command: String,
    pub(crate) url: String,
    pub(crate) args: Vec<String>,
    pub(crate) kind: String,
    pub(crate) transport_name: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) environment: BTreeMap<String, String>,
}

impl Entry {
    pub(crate) fn command_or_url(&self) -> String {
        if self.command.is_empty() {
            self.url.clone()
        } else {
            self.command.clone()
        }
    }

    pub(crate) fn transport(&self) -> String {
        match self.kind.to_ascii_lowercase().as_str() {
            "local" => "stdio".to_owned(),
            "remote" => "http".to_owned(),
            _ if !self.url.is_empty() && self.kind.is_empty() => "http".to_owned(),
            _ if !self.url.is_empty() => self.kind.clone(),
            _ if !self.transport_name.is_empty() => self.transport_name.clone(),
            _ => "stdio".to_owned(),
        }
    }

    pub(crate) fn merged_env(&self) -> BTreeMap<String, String> {
        let mut merged = self.environment.clone();
        merged.extend(self.env.clone());
        merged
    }
}

pub(crate) fn parse_config(data: &[u8], key: &str) -> Result<BTreeMap<String, Entry>, String> {
    use super::guard_scan_json::{self as json, Outcome};
    // Go: `strings.TrimSpace(string(data))`, then JSONC-strip the untrimmed text.
    let text = json::go_lossy(data);
    if text.trim().starts_with('{') {
        match json::entries(&strip_jsonc(&text), key) {
            Outcome::Entries(entries) => return Ok(entries),
            Outcome::Error(error) => return Err(error),
            Outcome::KeyNotFound => {}
        }
    }
    let root: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(data).map_err(|error| format!("parse YAML: {error}"))?;
    let Some((found, entries)) = lookup_yaml_entries(&root, key) else {
        return Err(format!("key not found: {key:?}"));
    };
    parse_yaml_entries(entries, found)
}

fn lookup_yaml_entries<'a>(
    root: &'a serde_yaml_ng::Value,
    key: &'a str,
) -> Option<(&'a str, &'a serde_yaml_ng::Value)> {
    let map = root.as_mapping()?;
    [key, "mcpServers", "mcp_servers", "mcp"]
        .into_iter()
        .find_map(|candidate| {
            map.get(serde_yaml_ng::Value::String(candidate.to_owned()))
                .map(|value| (candidate, value))
        })
}

fn parse_yaml_entries(
    value: &serde_yaml_ng::Value,
    key: &str,
) -> Result<BTreeMap<String, Entry>, String> {
    let object = value
        .as_mapping()
        .ok_or_else(|| format!("key {key:?} is not an object"))?;
    let mut entries = BTreeMap::new();
    for (name, value) in object {
        let Some(name) = name.as_str() else { continue };
        let Some(entry) = value.as_mapping() else {
            continue;
        };
        entries.insert(name.to_owned(), entry_from_yaml(entry));
    }
    Ok(entries)
}

fn entry_from_yaml(value: &serde_yaml_ng::Mapping) -> Entry {
    let string = |key: &str| {
        value
            .get(serde_yaml_ng::Value::String(key.to_owned()))
            .and_then(serde_yaml_ng::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let args = value
        .get(serde_yaml_ng::Value::String("args".to_owned()))
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map_or_else(Vec::new, |args| {
            args.iter()
                .filter_map(serde_yaml_ng::Value::as_str)
                .map(str::to_owned)
                .collect()
        });
    let map = |key: &str| {
        value
            .get(serde_yaml_ng::Value::String(key.to_owned()))
            .and_then(serde_yaml_ng::Value::as_mapping)
            .map_or_else(BTreeMap::new, |map| {
                map.iter()
                    .filter_map(|(key, value)| {
                        Some((key.as_str()?.to_owned(), value.as_str()?.to_owned()))
                    })
                    .collect()
            })
    };
    Entry {
        command: string("command"),
        url: string("url"),
        args,
        kind: string("type"),
        transport_name: string("transport"),
        env: map("env"),
        environment: BTreeMap::new(),
    }
}

fn strip_jsonc(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}
