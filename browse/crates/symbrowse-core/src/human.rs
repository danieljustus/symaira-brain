#![deny(unsafe_code)]

//! Dedicated human renderers for common daemon payloads (Go
//! `internal/output/human_renderers.go`).

use serde_json::{Map, Value};

use crate::{go_quote::quote, go_value};

type Fields = Map<String, Value>;

/// Renders `data` for people: a dedicated renderer when one matches, indented
/// JSON otherwise. The caller appends nothing; the result ends with a newline.
pub(crate) fn render_value(data: &Value) -> String {
    if let Value::Object(fields) = data
        && let Some(rendered) = render_payload(fields)
    {
        return rendered;
    }
    let mut out = go_value::marshal_indent(data);
    out.push('\n');
    out
}

fn render_payload(fields: &Fields) -> Option<String> {
    render_snapshot(fields)
        .or_else(|| render_navigation(fields))
        .or_else(|| render_state(fields))
        .or_else(|| render_cookies(fields))
        .or_else(|| render_tabs(fields))
}

fn string_field<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields.get(key)?.as_str()
}

fn integer_field(fields: &Fields, key: &str) -> Option<i64> {
    let number = fields.get(key)?.as_f64()?;
    (number == number.trunc() && number.abs() < 9.2e18).then_some(number as i64)
}

fn has_any(fields: &Fields, keys: &[&str]) -> bool {
    keys.iter().any(|key| fields.contains_key(*key))
}

fn render_navigation(fields: &Fields) -> Option<String> {
    let action = string_field(fields, "action").filter(|v| !v.is_empty())?;
    let url = string_field(fields, "url").filter(|v| !v.is_empty())?;
    let mut line = format!("{action} {url}");
    if let Some(status) = integer_field(fields, "http_status")
        && status > 0
    {
        line.push_str(&format!(" (HTTP {status})"));
    }
    line.push('\n');
    Some(line)
}

fn render_snapshot(fields: &Fields) -> Option<String> {
    if has_any(fields, &["added", "removed", "changed"]) {
        return Some(render_snapshot_diff(fields));
    }
    let tree = string_field(fields, "tree")?;
    if !fields.contains_key("refs") && !fields.contains_key("snapshot_id") {
        return None;
    }
    let heading = match string_field(fields, "snapshot_id") {
        Some(id) if !id.is_empty() => format!("{id} tree:"),
        _ => "tree:".to_owned(),
    };
    let mut out = heading;
    out.push('\n');
    let trimmed = tree.trim_end_matches('\n');
    if !trimmed.is_empty() {
        out.push_str(trimmed);
        out.push('\n');
    }
    if let Some(hint) = string_field(fields, "hint").filter(|v| !v.is_empty()) {
        out.push_str("hint: ");
        out.push_str(hint);
        out.push('\n');
    }
    Some(out)
}

fn render_snapshot_diff(fields: &Fields) -> String {
    let mut out = match string_field(fields, "snapshot_id") {
        Some(id) if !id.is_empty() => format!("{id} diff:"),
        _ => "snapshot diff:".to_owned(),
    };
    out.push('\n');
    for label in ["added", "removed", "changed"] {
        let Some(Value::Array(items)) = fields.get(label) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        out.push_str(label);
        out.push_str(":\n");
        for item in items {
            out.push_str("- ");
            out.push_str(&snapshot_item_text(item));
            out.push('\n');
        }
    }
    if let Some(hint) = string_field(fields, "hint").filter(|v| !v.is_empty()) {
        out.push_str("hint: ");
        out.push_str(hint);
        out.push('\n');
    }
    out
}

fn snapshot_item_text(item: &Value) -> String {
    let Value::Object(fields) = item else {
        return go_value::sprint(item);
    };
    for key in ["ref", "refkey"] {
        if let Some(reference) = string_field(fields, key).filter(|v| !v.is_empty()) {
            return reference.to_owned();
        }
    }
    let mut role = string_field(fields, "role").unwrap_or_default();
    let name = string_field(fields, "name").unwrap_or_default();
    if role.is_empty() {
        role = "node";
    }
    if name.is_empty() {
        role.to_owned()
    } else {
        format!("{role} {}", quote(name))
    }
}

fn string_list(fields: &Fields, key: &str) -> Option<Vec<String>> {
    match fields.get(key)? {
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

fn render_state(fields: &Fields) -> Option<String> {
    if let Some(saved) = string_field(fields, "saved") {
        return Some(render_state_operation("saved", saved, fields));
    }
    if let Some(loaded) = string_field(fields, "loaded") {
        return Some(render_state_operation("loaded", loaded, fields));
    }
    if let Some(cleared) = string_field(fields, "cleared") {
        return Some(format!("cleared: {cleared}\n"));
    }
    if let Some(removed) = string_list(fields, "removed") {
        return Some(render_list("removed", &removed));
    }
    if let Some(states) = string_list(fields, "states") {
        return Some(render_list("states", &states));
    }
    if let Some(Value::Object(metadata)) = fields.get("metadata") {
        return Some(render_state_metadata(metadata, "metadata:"));
    }
    if has_any(
        fields,
        &["schema_version", "saved_at", "expires_at", "key_source"],
    ) && fields.contains_key("origins")
    {
        return Some(render_state_metadata(fields, ""));
    }
    None
}

fn render_state_operation(operation: &str, name: &str, fields: &Fields) -> String {
    let mut out = format!("{operation}: {name}\n");
    if let Some(Value::Object(metadata)) = fields.get("metadata") {
        out.push_str(&render_state_metadata(metadata, "metadata:"));
    }
    out
}

fn human_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => {
            let float = number.as_f64().unwrap_or(0.0);
            if float == float.trunc() && float.abs() < 9.2e18 {
                format!("{}", float as i64)
            } else {
                format!("{float}")
            }
        }
        other => go_value::sprint(other),
    }
}

fn render_state_metadata(fields: &Fields, heading: &str) -> String {
    let mut out = String::new();
    if !heading.is_empty() {
        out.push_str(heading);
        out.push('\n');
    }
    for (key, label) in [
        ("name", "name"),
        ("schema_version", "schema version"),
        ("saved_at", "saved at"),
        ("expires_at", "expires at"),
        ("key_source", "key source"),
    ] {
        if let Some(value) = fields.get(key) {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(&human_scalar(value));
            out.push('\n');
        }
    }
    if let Some(Value::Array(origins)) = fields.get("origins") {
        out.push_str("origins:\n");
        for origin in origins {
            out.push_str("- ");
            out.push_str(&origin_text(origin));
            out.push('\n');
        }
    }
    out
}

fn origin_text(origin: &Value) -> String {
    let Value::Object(fields) = origin else {
        return go_value::sprint(origin);
    };
    format!(
        "{} (cookies={}, local={}, session={})",
        string_field(fields, "origin").unwrap_or_default(),
        integer_field(fields, "cookie_count").unwrap_or(0),
        integer_field(fields, "local_storage_keys").unwrap_or(0),
        integer_field(fields, "session_storage_keys").unwrap_or(0),
    )
}

fn render_cookies(fields: &Fields) -> Option<String> {
    let Value::Array(cookies) = fields.get("cookies")? else {
        return None;
    };
    let mut out = String::new();
    if let Some(origin) = string_field(fields, "origin").filter(|v| !v.is_empty()) {
        out.push_str("origin: ");
        out.push_str(origin);
        out.push('\n');
    }
    out.push_str("cookies:\n");
    for cookie in cookies {
        out.push_str("- ");
        out.push_str(&cookie_text(cookie));
        out.push('\n');
    }
    Some(out)
}

fn cookie_text(cookie: &Value) -> String {
    let Value::Object(fields) = cookie else {
        return go_value::sprint(cookie);
    };
    let name = string_field(fields, "name")
        .filter(|v| !v.is_empty())
        .unwrap_or("cookie");
    let mut parts = vec![name.to_owned()];
    for (key, label) in [
        ("domain", "domain"),
        ("path", "path"),
        ("same_site", "same_site"),
    ] {
        if let Some(value) = string_field(fields, key).filter(|v| !v.is_empty()) {
            parts.push(format!("{label}={value}"));
        }
    }
    for (key, label) in [
        ("secure", "secure"),
        ("http_only", "httpOnly"),
        ("session", "session"),
    ] {
        if fields.get(key).and_then(Value::as_bool) == Some(true) {
            parts.push(label.to_owned());
        }
    }
    parts.join(" ")
}

fn render_tabs(fields: &Fields) -> Option<String> {
    let Value::Array(tabs) = fields.get("tabs")? else {
        return None;
    };
    let mut out = String::new();
    if let Some(active) = string_field(fields, "active").filter(|v| !v.is_empty()) {
        out.push_str("active: ");
        out.push_str(active);
        out.push('\n');
    }
    out.push_str("tabs:\n");
    for tab in tabs {
        out.push_str("- ");
        out.push_str(&tab_text(tab));
        out.push('\n');
    }
    Some(out)
}

fn tab_text(tab: &Value) -> String {
    let Value::Object(fields) = tab else {
        return go_value::sprint(tab);
    };
    let id = string_field(fields, "id").unwrap_or_default();
    let label = string_field(fields, "label").unwrap_or_default();
    let url = string_field(fields, "url").unwrap_or_default();
    let mut parts = Vec::new();
    if !id.is_empty() {
        parts.push(id.to_owned());
    }
    if !label.is_empty() && label != id {
        parts.push(quote(label));
    }
    if !url.is_empty() {
        parts.push(url.to_owned());
    }
    if fields.get("active").and_then(Value::as_bool) == Some(true) {
        parts.push("[active]".to_owned());
    }
    if parts.is_empty() {
        "tab".to_owned()
    } else {
        parts.join(" ")
    }
}

fn render_list(label: &str, values: &[String]) -> String {
    let mut out = format!("{label}:\n");
    for value in values {
        out.push_str("- ");
        out.push_str(value);
        out.push('\n');
    }
    out
}
