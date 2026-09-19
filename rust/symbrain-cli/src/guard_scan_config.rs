//! Configuration parsing for the native `symbrain guard scan` inventory.

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Default)]
pub(super) struct Entry {
    pub(super) command: String,
    pub(super) url: String,
    pub(super) args: Vec<String>,
    pub(super) kind: String,
    pub(super) transport_name: String,
    pub(super) env: BTreeMap<String, String>,
    pub(super) environment: BTreeMap<String, String>,
}

impl Entry {
    pub(super) fn command_or_url(&self) -> String {
        if self.command.is_empty() {
            self.url.clone()
        } else {
            self.command.clone()
        }
    }

    pub(super) fn transport(&self) -> String {
        match self.kind.to_ascii_lowercase().as_str() {
            "local" => "stdio".to_owned(),
            "remote" => "http".to_owned(),
            _ if !self.url.is_empty() && self.kind.is_empty() => "http".to_owned(),
            _ if !self.url.is_empty() => self.kind.clone(),
            _ if !self.transport_name.is_empty() => self.transport_name.clone(),
            _ => "stdio".to_owned(),
        }
    }

    pub(super) fn merged_env(&self) -> BTreeMap<String, String> {
        let mut merged = self.environment.clone();
        merged.extend(self.env.clone());
        merged
    }
}

pub(super) fn parse_config(data: &[u8], key: &str) -> Result<BTreeMap<String, Entry>, String> {
    let trimmed = String::from_utf8_lossy(data).trim().to_owned();
    if trimmed.starts_with('{') {
        let stripped = strip_jsonc(&trimmed);
        let root: Value =
            serde_json::from_str(&stripped).map_err(|error| format!("parse JSON: {error}"))?;
        if let Some(entries) = lookup_entries(&root, key) {
            return parse_entries(entries);
        }
        if let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&trimmed)
            && let Some(entries) = lookup_yaml_entries(&doc, key)
        {
            return parse_yaml_entries(entries);
        }
        return Err(format!("key not found: {key:?}"));
    }
    let root: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(data).map_err(|error| format!("parse YAML: {error}"))?;
    let Some(entries) = lookup_yaml_entries(&root, key) else {
        return Err(format!("key not found: {key:?}"));
    };
    parse_yaml_entries(entries)
}

fn lookup_entries<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    let object = root.as_object()?;
    [key, "mcpServers", "mcp_servers", "mcp"]
        .into_iter()
        .find_map(|candidate| object.get(candidate))
}

fn lookup_yaml_entries<'a>(
    root: &'a serde_yaml_ng::Value,
    key: &str,
) -> Option<&'a serde_yaml_ng::Value> {
    let map = root.as_mapping()?;
    [key, "mcpServers", "mcp_servers", "mcp"]
        .into_iter()
        .find_map(|candidate| map.get(serde_yaml_ng::Value::String(candidate.to_owned())))
}

fn parse_entries(value: &Value) -> Result<BTreeMap<String, Entry>, String> {
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    let object = value
        .as_object()
        .ok_or_else(|| "parse entries: expected object".to_owned())?;
    let mut entries = BTreeMap::new();
    for (name, value) in object {
        let entry = match value {
            Value::Null => Entry::default(),
            Value::Object(entry) => entry_from_json(entry)
                .map_err(|error| format!("parse entries: server {name:?}: {error}"))?,
            _ => return Err(format!("parse entries: expected object for {name:?}")),
        };
        entries.insert(name.clone(), entry);
    }
    Ok(entries)
}

fn entry_from_json(value: &serde_json::Map<String, Value>) -> Result<Entry, String> {
    let args = match value.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(args)) => args
            .iter()
            .enumerate()
            .map(|(index, value)| match value {
                Value::Null => Ok(String::new()),
                Value::String(value) => Ok(value.clone()),
                _ => Err(format!("args[{index}] must be a string")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("args must be an array".to_owned()),
    };
    Ok(Entry {
        command: string_field(value, "command")?,
        url: string_field(value, "url")?,
        args,
        kind: string_field(value, "type")?,
        transport_name: string_field(value, "transport")?,
        env: map_field(value.get("env"), "env")?,
        environment: map_field(value.get("environment"), "environment")?,
    })
}

fn string_field(value: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn map_field(value: Option<&Value>, field: &str) -> Result<BTreeMap<String, String>, String> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let Some(map) = value.as_object() else {
        if value.is_null() {
            return Ok(BTreeMap::new());
        }
        return Err(format!("{field} must be an object"));
    };
    map.iter()
        .map(|(key, value)| match value {
            Value::Null => Ok((key.clone(), String::new())),
            Value::String(value) => Ok((key.clone(), value.clone())),
            _ => Err(format!("{field} value {key:?} must be a string")),
        })
        .collect()
}

fn parse_yaml_entries(value: &serde_yaml_ng::Value) -> Result<BTreeMap<String, Entry>, String> {
    let object = value
        .as_mapping()
        .ok_or_else(|| "key is not an object".to_owned())?;
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
