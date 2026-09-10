//! Undecoded key collection and warnings for profile TOML.

use toml_edit::{Item, Table, Value};

use crate::profile::parse::KNOWN_SERVER_FIELDS;

pub(crate) fn collect_declared_keys(table: &Table, prefix: &str, out: &mut Vec<String>) {
    for (k, item) in table {
        let full_path = if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        };
        match item {
            Item::Table(sub_table) => {
                if !sub_table.is_implicit() {
                    out.push(full_path.clone());
                }
                collect_declared_keys(sub_table, &full_path, out);
            }
            Item::Value(value) => match value {
                Value::InlineTable(inline_table) => {
                    out.push(full_path.clone());
                    collect_inline_keys(inline_table, &full_path, out);
                }
                _ => {
                    out.push(full_path);
                }
            },
            Item::ArrayOfTables(aot) => {
                for sub_table in aot {
                    out.push(full_path.clone());
                    collect_declared_keys(sub_table, &full_path, out);
                }
            }
            Item::None => {}
        }
    }
}

pub(crate) fn collect_inline_keys(
    inline_table: &toml_edit::InlineTable,
    prefix: &str,
    out: &mut Vec<String>,
) {
    for (k, value) in inline_table {
        let full_path = format!("{prefix}.{k}");
        match value {
            Value::InlineTable(sub_inline) => {
                if !sub_inline.is_dotted() {
                    out.push(full_path.clone());
                }
                collect_inline_keys(sub_inline, &full_path, out);
            }
            _ => {
                out.push(full_path);
            }
        }
    }
}

pub(crate) fn is_key_decoded(key: &str) -> bool {
    if key == "profile" || key == "profile.name" || key == "profile.description" {
        return true;
    }
    if key == "audit" || key == "audit.enabled" || key == "audit.verbose" {
        return true;
    }
    if key == "servers" {
        return true;
    }
    if let Some(rest) = key.strip_prefix("servers.") {
        if let Some((_alias, field)) = rest.split_once('.') {
            if KNOWN_SERVER_FIELDS.contains(&field) {
                return true;
            }
        } else {
            // key is servers.<alias>
            return true;
        }
    }
    false
}
