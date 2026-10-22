pub(crate) use crate::json_parser::{Object, Value};
use crate::{Entry, ServerInfo};

pub(crate) fn marshal(value: &Value) -> Vec<u8> {
    let mut output = String::new();
    marshal_value(&mut output, value);
    output.push('\n');
    output.into_bytes()
}

/// An open container of [`marshal_value`] and the element it is writing.
enum Frame<'a> {
    Object(&'a Object, usize),
    Array(&'a [Value], usize),
}

fn push_indent(output: &mut String, depth: usize) {
    output.extend(std::iter::repeat_n(' ', depth * 2));
}

/// Ends the element the innermost container just wrote: the separating
/// comma (all but the last) and the line break.
fn finish_element(output: &mut String, stack: &mut [Frame<'_>]) {
    let Some(frame) = stack.last_mut() else {
        return;
    };
    let (index, len) = match frame {
        Frame::Object(object, index) => (index, object.keys.len()),
        Frame::Array(values, index) => (index, values.len()),
    };
    if *index + 1 != len {
        output.push(',');
    }
    output.push('\n');
    *index += 1;
}

/// Writes `value` with two-space indentation like Go's `marshalJSONValue`.
///
/// Nesting is walked with an explicit stack: Go's decoder accepts any depth
/// below the configuration size cap, so the native stack must not bound it.
fn marshal_value(output: &mut String, root: &Value) {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut pending = Some(root);
    loop {
        if let Some(value) = pending.take() {
            let opened = match value {
                Value::Object(object) if !object.keys.is_empty() => {
                    output.push_str("{\n");
                    stack.push(Frame::Object(object, 0));
                    true
                }
                Value::Object(_) => {
                    output.push_str("{}");
                    false
                }
                Value::Array(values) if !values.is_empty() => {
                    output.push_str("[\n");
                    stack.push(Frame::Array(values, 0));
                    true
                }
                Value::Array(_) => {
                    output.push_str("[]");
                    false
                }
                Value::String(value) => {
                    output
                        .push_str(&serde_json::to_string(value).expect("strings are serializable"));
                    false
                }
                Value::Number(value) => {
                    output.push_str(value);
                    false
                }
                Value::Bool(value) => {
                    output.push_str(if *value { "true" } else { "false" });
                    false
                }
                Value::Null => {
                    output.push_str("null");
                    false
                }
            };
            if !opened {
                finish_element(output, &mut stack);
            }
            continue;
        }
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return;
        };
        match frame {
            Frame::Object(object, index) => {
                let object: &Object = object;
                if let Some(key) = object.keys.get(*index) {
                    push_indent(output, depth);
                    output.push_str(&serde_json::to_string(key).expect("keys are serializable"));
                    output.push_str(": ");
                    pending = Some(&object.values[key]);
                } else {
                    stack.pop();
                    push_indent(output, depth - 1);
                    output.push('}');
                    finish_element(output, &mut stack);
                }
            }
            Frame::Array(values, index) => {
                let values: &[Value] = values;
                if let Some(value) = values.get(*index) {
                    push_indent(output, depth);
                    pending = Some(value);
                } else {
                    stack.pop();
                    push_indent(output, depth - 1);
                    output.push(']');
                    finish_element(output, &mut stack);
                }
            }
        }
    }
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
