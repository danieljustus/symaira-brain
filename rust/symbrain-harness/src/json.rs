pub(crate) use crate::json_parser::{Object, Value};
use crate::{Entry, ServerInfo};

pub(crate) use crate::json_parser::parse;

pub(crate) fn marshal(value: &Value) -> Vec<u8> {
    let mut output = String::new();
    marshal_value(&mut output, value, "");
    output.push('\n');
    output.into_bytes()
}

fn marshal_value(output: &mut String, value: &Value, prefix: &str) {
    match value {
        Value::Object(object) => marshal_object(output, object, prefix),
        Value::Array(values) => marshal_array(output, values, prefix),
        Value::String(value) => {
            output.push_str(&serde_json::to_string(value).expect("strings are serializable"));
        }
        Value::Number(value) => output.push_str(value),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Null => output.push_str("null"),
    }
}
fn marshal_object(output: &mut String, object: &Object, prefix: &str) {
    if object.keys.is_empty() {
        output.push_str("{}");
        return;
    }
    output.push_str("{\n");
    let child = format!("{prefix}  ");
    for (index, key) in object.keys.iter().enumerate() {
        output.push_str(&child);
        output.push_str(&serde_json::to_string(key).expect("keys are serializable"));
        output.push_str(": ");
        marshal_value(output, &object.values[key], &child);
        if index + 1 != object.keys.len() {
            output.push(',');
        }
        output.push('\n');
    }
    output.push_str(prefix);
    output.push('}');
}
fn marshal_array(output: &mut String, values: &[Value], prefix: &str) {
    if values.is_empty() {
        output.push_str("[]");
        return;
    }
    output.push_str("[\n");
    let child = format!("{prefix}  ");
    for (index, value) in values.iter().enumerate() {
        output.push_str(&child);
        marshal_value(output, value, &child);
        if index + 1 != values.len() {
            output.push(',');
        }
        output.push('\n');
    }
    output.push_str(prefix);
    output.push(']');
}

pub(crate) fn entry(value: &Value) -> Option<Entry> {
    let Value::Object(object) = value else {
        return None;
    };
    let command = match object.get("command") {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    };
    let args = object
        .get("args")
        .and_then(|value| match value {
            Value::Array(values) => Some(
                values
                    .iter()
                    .filter_map(|value| match value {
                        Value::String(value) => Some(value.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    Some(Entry { command, args })
}

pub(crate) fn server_info(name: &str, value: &Value) -> Option<ServerInfo> {
    let Value::Object(object) = value else {
        return None;
    };
    let mut fields = Vec::new();
    for key in ["command", "url", "transport", "type"] {
        if let Some(Value::String(value)) = object.get(key) {
            fields.push((
                key.to_owned(),
                crate::server_info::ValueKind::String(value.clone()),
            ));
        }
    }
    if let Some(Value::Array(values)) = object.get("args") {
        fields.push((
            "args".into(),
            crate::server_info::ValueKind::Strings(
                values
                    .iter()
                    .filter_map(|value| match value {
                        Value::String(value) => Some(value.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
        ));
    }
    if let Some(Value::Object(env)) = object.get("env") {
        fields.push((
            "env".into(),
            crate::server_info::ValueKind::Names(env.keys.clone()),
        ));
    }
    Some(ServerInfo::from_fields(name, &fields))
}

pub(crate) fn entry_value(entry: Entry) -> Value {
    Value::Object({
        let mut object = Object::new();
        object.set("command", Value::String(entry.command));
        object.set(
            "args",
            Value::Array(entry.args.into_iter().map(Value::String).collect()),
        );
        object
    })
}

pub(crate) fn ensure_server_map(root: &mut Value, key: &str) {
    let Value::Object(root) = root else { return };
    if !matches!(root.get(key), Some(Value::Object(_))) {
        root.set(key, Value::Object(Object::new()));
    }
}

pub(crate) fn remove_empty_server_map(root: &mut Value, key: &str, removed: bool) {
    if !removed {
        return;
    }
    match root {
        Value::Object(root)
            if root.get(key).is_some_and(
                |value| matches!(value, Value::Object(map) if map.keys.is_empty()),
            ) =>
        {
            root.remove(key);
        }
        _ => {}
    }
}

pub(crate) fn server_map_mut<'a>(root: &'a mut Value, key: &str) -> Option<&'a mut Object> {
    match root {
        Value::Object(root) => match root.get(key) {
            Some(Value::Object(_)) => match root.values.get_mut(key) {
                Some(Value::Object(value)) => Some(value),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}
pub(crate) fn server_map<'a>(root: &'a Value, key: &str) -> Option<&'a Object> {
    match root {
        Value::Object(root) => match root.get(key) {
            Some(Value::Object(value)) => Some(value),
            _ => None,
        },
        _ => None,
    }
}
