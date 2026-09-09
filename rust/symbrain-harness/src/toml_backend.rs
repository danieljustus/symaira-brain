use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::server_info::ValueKind;
use crate::{Entry, ServerInfo};

pub(crate) fn server<'a>(document: &'a DocumentMut, key: &str, name: &str) -> Option<&'a Item> {
    document.get(key)?.as_table()?.get(name)
}

pub(crate) fn entry(item: &Item) -> Option<Entry> {
    let table = item.as_table()?;
    let command = table
        .get("command")
        .and_then(Item::as_str)
        .unwrap_or_default()
        .to_owned();
    let args = table
        .get("args")
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Entry { command, args })
}

pub(crate) fn server_info(name: &str, item: &Item) -> Option<ServerInfo> {
    let table = item.as_table()?;
    let mut fields = Vec::new();
    for key in ["command", "url", "transport", "type"] {
        if let Some(value) = table.get(key).and_then(Item::as_str) {
            fields.push((key.to_owned(), ValueKind::String(value.to_owned())));
        }
    }
    if let Some(array) = table.get("args").and_then(Item::as_array) {
        fields.push((
            "args".into(),
            ValueKind::Strings(
                array
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
        ));
    }
    if let Some(env) = table.get("env").and_then(Item::as_table) {
        fields.push((
            "env".into(),
            ValueKind::Names(env.iter().map(|(key, _)| key.to_owned()).collect()),
        ));
    }
    Some(ServerInfo::from_fields(name, &fields))
}

pub(crate) fn server_names(document: &DocumentMut, key: &str) -> Vec<String> {
    document
        .get(key)
        .and_then(Item::as_table)
        .map(|table| table.iter().map(|(name, _)| name.to_owned()).collect())
        .unwrap_or_default()
}

pub(crate) fn set_server(document: &mut DocumentMut, key: &str, name: &str, entry: Entry) {
    if document.get(key).and_then(Item::as_table).is_none() {
        document[key] = Item::Table(Table::new());
    }
    let mut table = Table::new();
    table.decor_mut().set_prefix("  ");
    let mut args = Array::new();
    for arg in entry.args {
        args.push(arg);
    }
    table["args"] = Item::Value(Value::Array(args));
    if let Some(mut key) = table.key_mut("args") {
        key.leaf_decor_mut().set_prefix("    ");
    }
    table["command"] = toml_edit::value(entry.command);
    if let Some(mut key) = table.key_mut("command") {
        key.leaf_decor_mut().set_prefix("    ");
    }
    document[key][name] = Item::Table(table);
}

pub(crate) fn remove_server(document: &mut DocumentMut, key: &str, name: &str) -> bool {
    let Some(table) = document.get_mut(key).and_then(Item::as_table_mut) else {
        return false;
    };
    if table.remove(name).is_none() {
        return false;
    }
    if table.is_empty() {
        document.remove(key);
    }
    true
}
