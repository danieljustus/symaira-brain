//! TOML decoding and undecoded key tracking for profiles.

use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::error::ProfileError;
use crate::profile::resolve::{RawServer, resolve_servers};
use crate::profile::undecoded::{collect_declared_keys, is_key_decoded};
use crate::profile::{AuditConfig, Profile};

pub const KNOWN_SERVER_FIELDS: [&str; 10] = [
    "enabled",
    "mode",
    "tools_allow",
    "tools_deny",
    "command",
    "args",
    "url",
    "access",
    "tools_read",
    "tools_write",
];

/// Decodes just the `[profile].name` field from a TOML string.
///
/// # Errors
///
/// Returns [`ProfileError::ParseFailed`] if TOML syntax is invalid.
pub fn parse_meta_name(data: &str) -> Result<String, ProfileError> {
    let doc = data
        .parse::<DocumentMut>()
        .map_err(|err| ProfileError::ParseFailed {
            name: None,
            path: None,
            message: err.to_string(),
        })?;

    if let Some(item) = doc.get("profile") {
        let table =
            ItemRef::from_item(item)
                .as_table()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: None,
                    path: None,
                    message: "expected a table for [profile]".to_string(),
                })?;
        if let Some(name_item) = table.get_item("name") {
            let name_str = name_item
                .as_str()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: None,
                    path: None,
                    message: "expected a string for profile.name".to_string(),
                })?;
            return Ok(name_str.to_string());
        }
    }

    Ok(String::new())
}

/// Parses and validates a profile from raw TOML content.
///
/// # Errors
///
/// Returns [`ProfileError`] if TOML parsing fails, profile name mismatches,
/// or server configurations fail validation.
pub fn parse(name: &str, data: &str) -> Result<Profile, ProfileError> {
    crate::profile::validate::validate_name(name)?;

    let doc = data
        .parse::<DocumentMut>()
        .map_err(|err| ProfileError::ParseFailed {
            name: Some(name.to_string()),
            path: None,
            message: err.to_string(),
        })?;

    let (profile_name, profile_description) = parse_profile_meta(name, &doc)?;
    if profile_name != name {
        return Err(ProfileError::NameMismatch {
            expected: name.to_string(),
            actual: profile_name.to_string(),
        });
    }

    let audit = parse_audit(name, &doc)?;
    let raw_servers = parse_raw_servers(name, &doc)?;

    // Resolve servers with presets, defaulting, and foreign validation.
    let (servers, server_warnings) = resolve_servers(&raw_servers).map_err(|err| match err {
        ProfileError::InvalidServer { message, .. } => ProfileError::InvalidServer {
            name: name.to_string(),
            message,
        },
        other => other,
    })?;

    // Collect undecoded keys.
    let mut declared_keys = Vec::new();
    collect_declared_keys(doc.as_table(), "", &mut declared_keys);

    let mut warnings: Vec<String> = declared_keys
        .into_iter()
        .filter(|k| !is_key_decoded(k))
        .map(|k| format!("unknown key \"{k}\""))
        .collect();
    warnings.extend(server_warnings);
    warnings.sort();

    Ok(Profile {
        name: profile_name.to_string(),
        description: profile_description.to_string(),
        servers,
        audit,
        warnings,
    })
}

fn parse_profile_meta<'a>(
    name: &str,
    doc: &'a DocumentMut,
) -> Result<(&'a str, &'a str), ProfileError> {
    let mut profile_name = "";
    let mut profile_description = "";

    if let Some(item) = doc.get("profile") {
        let table =
            ItemRef::from_item(item)
                .as_table()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: Some(name.to_string()),
                    path: None,
                    message: "expected a table for [profile]".to_string(),
                })?;
        if let Some(n_item) = table.get_item("name") {
            profile_name = n_item.as_str().ok_or_else(|| ProfileError::ParseFailed {
                name: Some(name.to_string()),
                path: None,
                message: "expected a string for profile.name".to_string(),
            })?;
        }
        if let Some(d_item) = table.get_item("description") {
            profile_description = d_item.as_str().ok_or_else(|| ProfileError::ParseFailed {
                name: Some(name.to_string()),
                path: None,
                message: "expected a string for profile.description".to_string(),
            })?;
        }
    }
    Ok((profile_name, profile_description))
}

fn parse_audit(name: &str, doc: &DocumentMut) -> Result<AuditConfig, ProfileError> {
    if let Some(item) = doc.get("audit") {
        let table =
            ItemRef::from_item(item)
                .as_table()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: Some(name.to_string()),
                    path: None,
                    message: "expected a table for [audit]".to_string(),
                })?;
        let enabled = if let Some(en_item) = table.get_item("enabled") {
            en_item.as_bool().ok_or_else(|| ProfileError::ParseFailed {
                name: Some(name.to_string()),
                path: None,
                message: "expected a boolean for audit.enabled".to_string(),
            })?
        } else {
            true
        };
        let verbose = if let Some(verbose_item) = table.get_item("verbose") {
            verbose_item
                .as_bool()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: Some(name.to_string()),
                    path: None,
                    message: "expected a boolean for audit.verbose".to_string(),
                })?
        } else {
            false
        };
        return Ok(AuditConfig { enabled, verbose });
    }
    Ok(AuditConfig::default())
}

fn parse_raw_servers(
    name: &str,
    doc: &DocumentMut,
) -> Result<BTreeMap<String, RawServer>, ProfileError> {
    let mut raw_servers = BTreeMap::new();
    if let Some(item) = doc.get("servers") {
        let servers_table =
            ItemRef::from_item(item)
                .as_table()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: Some(name.to_string()),
                    path: None,
                    message: "expected a table for [servers]".to_string(),
                })?;
        for (alias, server_item) in servers_table.iter() {
            let server_ref = server_item
                .as_table()
                .ok_or_else(|| ProfileError::ParseFailed {
                    name: Some(name.to_string()),
                    path: None,
                    message: format!("expected a table for server \"{alias}\""),
                })?;
            raw_servers.insert(
                alias.to_string(),
                parse_raw_server(name, alias, server_ref)?,
            );
        }
    }
    Ok(raw_servers)
}

fn parse_raw_server(
    profile_name: &str,
    alias: &str,
    table: TableRef<'_>,
) -> Result<RawServer, ProfileError> {
    let enabled = if let Some(item) = table.get_item("enabled") {
        Some(item.as_bool().ok_or_else(|| ProfileError::ParseFailed {
            name: Some(profile_name.to_string()),
            path: None,
            message: format!("expected a boolean for servers.{alias}.enabled"),
        })?)
    } else {
        None
    };

    let mode = parse_str_field(
        profile_name,
        &format!("servers.{alias}.mode"),
        table.get_item("mode"),
    )?;
    let command = parse_str_field(
        profile_name,
        &format!("servers.{alias}.command"),
        table.get_item("command"),
    )?;
    let url = parse_str_field(
        profile_name,
        &format!("servers.{alias}.url"),
        table.get_item("url"),
    )?;
    let access = parse_str_field(
        profile_name,
        &format!("servers.{alias}.access"),
        table.get_item("access"),
    )?;

    let tools_allow = parse_str_array_strict(
        profile_name,
        &format!("servers.{alias}.tools_allow"),
        table.get_item("tools_allow"),
    )?;
    let tools_deny = parse_str_array_strict(
        profile_name,
        &format!("servers.{alias}.tools_deny"),
        table.get_item("tools_deny"),
    )?;
    let args = parse_str_array_strict(
        profile_name,
        &format!("servers.{alias}.args"),
        table.get_item("args"),
    )?;
    let tools_read = parse_str_array_strict(
        profile_name,
        &format!("servers.{alias}.tools_read"),
        table.get_item("tools_read"),
    )?;
    let tools_write = parse_str_array_strict(
        profile_name,
        &format!("servers.{alias}.tools_write"),
        table.get_item("tools_write"),
    )?;

    Ok(RawServer {
        enabled,
        mode,
        tools_allow,
        tools_deny,
        command,
        args,
        url,
        access,
        tools_read,
        tools_write,
    })
}

fn parse_str_field(
    profile_name: &str,
    field_name: &str,
    item: Option<ItemRef<'_>>,
) -> Result<String, ProfileError> {
    if let Some(item) = item {
        item.as_str()
            .map(String::from)
            .ok_or_else(|| ProfileError::ParseFailed {
                name: Some(profile_name.to_string()),
                path: None,
                message: format!("expected a string for {field_name}"),
            })
    } else {
        Ok(String::new())
    }
}

fn parse_str_array_strict(
    profile_name: &str,
    field_name: &str,
    item: Option<ItemRef<'_>>,
) -> Result<Vec<String>, ProfileError> {
    let Some(item) = item else {
        return Ok(Vec::new());
    };
    let arr = item.as_array().ok_or_else(|| ProfileError::ParseFailed {
        name: Some(profile_name.to_string()),
        path: None,
        message: format!("expected an array for {field_name}"),
    })?;
    let mut res = Vec::with_capacity(arr.len());
    for val in arr {
        let s = val.as_str().ok_or_else(|| ProfileError::ParseFailed {
            name: Some(profile_name.to_string()),
            path: None,
            message: format!("expected string element in {field_name} array"),
        })?;
        res.push(s.to_string());
    }
    Ok(res)
}

#[derive(Copy, Clone)]
enum TableRef<'a> {
    Table(&'a Table),
    Inline(&'a toml_edit::InlineTable),
}

#[derive(Copy, Clone)]
enum ItemRef<'a> {
    Item(&'a Item),
    Value(&'a Value),
}

impl<'a> ItemRef<'a> {
    const fn from_item(item: &'a Item) -> Self {
        Self::Item(item)
    }

    fn as_table(self) -> Option<TableRef<'a>> {
        match self {
            Self::Item(item) => match item {
                Item::Table(t) => Some(TableRef::Table(t)),
                Item::Value(Value::InlineTable(it)) => Some(TableRef::Inline(it)),
                _ => None,
            },
            Self::Value(v) => match v {
                Value::InlineTable(it) => Some(TableRef::Inline(it)),
                _ => None,
            },
        }
    }

    fn as_str(self) -> Option<&'a str> {
        match self {
            Self::Item(item) => item.as_str(),
            Self::Value(v) => v.as_str(),
        }
    }

    fn as_bool(self) -> Option<bool> {
        match self {
            Self::Item(item) => item.as_bool(),
            Self::Value(v) => v.as_bool(),
        }
    }

    fn as_array(self) -> Option<&'a toml_edit::Array> {
        match self {
            Self::Item(item) => item.as_array(),
            Self::Value(Value::Array(arr)) => Some(arr),
            Self::Value(_) => None,
        }
    }
}

impl<'a> TableRef<'a> {
    fn get_item(self, key: &str) -> Option<ItemRef<'a>> {
        match self {
            Self::Table(t) => t.get(key).map(ItemRef::Item),
            Self::Inline(it) => it.get(key).map(ItemRef::Value),
        }
    }

    fn iter(self) -> Box<dyn Iterator<Item = (&'a str, ItemRef<'a>)> + 'a> {
        match self {
            Self::Table(t) => Box::new(t.iter().map(|(k, v)| (k, ItemRef::Item(v)))),
            Self::Inline(it) => Box::new(it.iter().map(|(k, v)| (k, ItemRef::Value(v)))),
        }
    }
}
