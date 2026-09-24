#![deny(unsafe_code)]

//! Unified JSON, YAML and human output envelope contracts.

use icu_properties::{props::GeneralCategory, CodePointMapData};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ErrorCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Text,
    Json,
    Yaml,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Warning {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#ref: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub excerpt: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_user_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resume_hint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Envelope {
    pub success: bool,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

impl Envelope {
    #[must_use]
    pub fn ok(data: Value, warnings: Vec<Warning>) -> Self {
        Self {
            success: true,
            data,
            warnings,
            error: None,
        }
    }

    #[must_use]
    pub fn failure(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: Value::Null,
            warnings: Vec::new(),
            error: Some(ErrorPayload {
                code,
                message: message.into(),
                hint: String::new(),
                details: None,
                retryable: None,
                requires_user_confirmation: None,
                resume_hint: String::new(),
            }),
        }
    }

    /// Renders the selected public output format with a trailing newline.
    pub fn render(&self, format: Format) -> Result<String, RenderError> {
        match format {
            Format::Json => {
                let mut output = serde_json::to_string(self).map_err(RenderError::Json)?;
                output.push('\n');
                Ok(output)
            }
            Format::Yaml => render_yaml(self),
            Format::Text => Ok(render_human(self)),
        }
    }
}

#[derive(Debug)]
pub enum RenderError {
    Json(serde_json::Error),
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "serialise envelope as json: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

fn render_yaml(envelope: &Envelope) -> Result<String, RenderError> {
    let mut output = format!("success: {}\n", envelope.success);
    write_yaml_field(&mut output, "data", &envelope.data, 0);
    write_yaml_warnings(&mut output, &envelope.warnings);
    write_yaml_error(&mut output, envelope.error.as_ref())?;
    Ok(output)
}

fn write_yaml_warnings(output: &mut String, warnings: &[Warning]) {
    if warnings.is_empty() {
        output.push_str("warnings: []\n");
        return;
    }
    output.push_str("warnings:\n");
    for warning in warnings {
        output.push_str("    - kind: ");
        output.push_str(&yaml_string(&warning.kind));
        output.push('\n');
        for (key, value) in [
            ("severity", warning.severity.as_str()),
            ("message", warning.message.as_str()),
            ("ref", warning.r#ref.as_str()),
            ("excerpt", warning.excerpt.as_str()),
        ] {
            if !value.is_empty() {
                output.push_str("      ");
                output.push_str(key);
                output.push_str(": ");
                output.push_str(&yaml_string(value));
                output.push('\n');
            }
        }
    }
}

fn write_yaml_error(output: &mut String, error: Option<&ErrorPayload>) -> Result<(), RenderError> {
    let Some(error) = error else {
        output.push_str("error: null\n");
        return Ok(());
    };
    output.push_str("error:\n");
    let code = serde_json::to_value(error.code).map_err(RenderError::Json)?;
    write_yaml_field(output, "code", &code, 4);
    write_yaml_field(output, "message", &Value::String(error.message.clone()), 4);
    if !error.hint.is_empty() {
        write_yaml_field(output, "hint", &Value::String(error.hint.clone()), 4);
    }
    write_yaml_field(
        output,
        "details",
        error
            .details
            .as_ref()
            .unwrap_or(&Value::Object(Default::default())),
        4,
    );
    write_yaml_field(
        output,
        "retryable",
        &error.retryable.map_or(Value::Null, Value::Bool),
        4,
    );
    write_yaml_field(
        output,
        "requiresuserconfirmation",
        &error
            .requires_user_confirmation
            .map_or(Value::Null, Value::Bool),
        4,
    );
    write_yaml_field(
        output,
        "resumehint",
        &Value::String(error.resume_hint.clone()),
        4,
    );
    Ok(())
}

pub(crate) fn write_yaml_field(output: &mut String, key: &str, value: &Value, indent: usize) {
    output.push_str(&" ".repeat(indent));
    output.push_str(&yaml_key(key));
    output.push(':');
    match value {
        Value::Object(values) if !values.is_empty() => {
            output.push('\n');
            for (child_key, child_value) in values {
                write_yaml_field(output, child_key, child_value, indent + 4);
            }
        }
        Value::Array(values) if !values.is_empty() => {
            output.push('\n');
            for item in values {
                write_yaml_sequence_item(output, item, indent + 4);
            }
        }
        Value::Object(_) => output.push_str(" {}\n"),
        Value::Array(_) => output.push_str(" []\n"),
        _ => {
            output.push(' ');
            output.push_str(&yaml_scalar(value));
            output.push('\n');
        }
    }
}

pub(crate) fn write_yaml_sequence_item(output: &mut String, value: &Value, indent: usize) {
    output.push_str(&" ".repeat(indent));
    output.push('-');
    match value {
        Value::Object(values) if !values.is_empty() => {
            let mut fields = values.iter();
            if let Some((key, first)) = fields.next() {
                output.push(' ');
                output.push_str(&yaml_key(key));
                output.push(':');
                if first.is_object() || first.is_array() {
                    output.push('\n');
                    write_yaml_children(output, first, indent + 4);
                } else {
                    output.push(' ');
                    output.push_str(&yaml_scalar(first));
                    output.push('\n');
                }
            }
            for (key, child) in fields {
                write_yaml_field(output, key, child, indent + 2);
            }
        }
        _ => {
            output.push(' ');
            output.push_str(&yaml_scalar(value));
            output.push('\n');
        }
    }
}

fn write_yaml_children(output: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Object(values) => {
            for (key, child) in values {
                write_yaml_field(output, key, child, indent);
            }
        }
        Value::Array(values) => {
            for child in values {
                write_yaml_sequence_item(output, child, indent);
            }
        }
        _ => {}
    }
}

fn yaml_key(value: &str) -> String {
    if needs_yaml_quotes(value) {
        serde_json::to_string(value).expect("string serialization cannot fail")
    } else {
        value.to_owned()
    }
}

pub(crate) fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => yaml_string(value),
        Value::Array(_) | Value::Object(_) => "null".to_owned(),
    }
}

fn yaml_string(value: &str) -> String {
    if value.starts_with('@') || value.starts_with('`') {
        return format!("'{}'", value.replace('\'', "''"));
    }
    if needs_yaml_quotes(value) {
        return serde_json::to_string(value).expect("string serialization cannot fail");
    }
    value.to_owned()
}

fn needs_yaml_quotes(value: &str) -> bool {
    if value.is_empty() || value.contains(['\n', '\r', '\t']) || value.contains(": ") {
        return true;
    }
    matches!(
        value.to_ascii_lowercase().as_str(),
        "y" | "yes" | "n" | "no" | "true" | "false" | "on" | "off" | "null" | "~"
    )
}

fn render_human(envelope: &Envelope) -> String {
    if !envelope.success {
        return match &envelope.error {
            Some(error) => format!("{}\n", error.message),
            None => "error\n".to_owned(),
        };
    }
    if envelope.data.is_null() {
        return "ok\n".to_owned();
    }
    if let Some(text) = envelope.data.as_str() {
        return format!("{text}\n");
    }
    if let Some(marker) = truncation_marker(&envelope.data) {
        return format!(
            "{}\n\n… [truncated: {} of {} tokens] …\n\n{}\n\nfull output: {}\n",
            marker.head, marker.tokens_returned, marker.tokens_total, marker.foot, marker.hint
        );
    }
    if let Some(fields) = envelope.data.as_object() {
        if let Some(rendered) = render_human_payload(fields) {
            return rendered;
        }
    }
    let mut output = serde_json::to_string_pretty(&envelope.data)
        .expect("serde_json::Value serialization cannot fail");
    output.push('\n');
    output
}

fn render_human_payload(fields: &serde_json::Map<String, Value>) -> Option<String> {
    if ["added", "removed", "changed"]
        .iter()
        .any(|key| fields.contains_key(*key))
    {
        return Some(render_snapshot_diff(fields));
    }
    if let (Some(tree), true) = (
        fields.get("tree").and_then(Value::as_str),
        fields.contains_key("refs") || fields.contains_key("snapshot_id"),
    ) {
        let mut output = fields
            .get("snapshot_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map_or_else(|| "tree:\n".to_owned(), |id| format!("{id} tree:\n"));
        let tree = tree.trim_end_matches('\n');
        if !tree.is_empty() {
            output.push_str(tree);
            output.push('\n');
        }
        if let Some(hint) = fields
            .get("hint")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            output.push_str("hint: ");
            output.push_str(hint);
            output.push('\n');
        }
        return Some(output);
    }
    if let (Some(action), Some(url)) = (
        fields
            .get("action")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()),
        fields
            .get("url")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()),
    ) {
        let mut output = format!("{action} {url}");
        if let Some(status) = fields
            .get("http_status")
            .and_then(json_integer)
            .filter(|n| *n > 0)
        {
            output.push_str(&format!(" (HTTP {status})"));
        }
        output.push('\n');
        return Some(output);
    }
    if let Some(rendered) = render_state_payload(fields) {
        return Some(rendered);
    }
    if let Some(cookies) = fields.get("cookies").and_then(Value::as_array) {
        let mut output = String::new();
        if let Some(origin) = fields
            .get("origin")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            output.push_str("origin: ");
            output.push_str(origin);
            output.push('\n');
        }
        output.push_str("cookies:\n");
        for cookie in cookies {
            output.push_str("- ");
            output.push_str(&cookie_text(cookie));
            output.push('\n');
        }
        return Some(output);
    }
    if let Some(tabs) = fields.get("tabs").and_then(Value::as_array) {
        let mut output = String::new();
        if let Some(active) = fields
            .get("active")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            output.push_str("active: ");
            output.push_str(active);
            output.push('\n');
        }
        output.push_str("tabs:\n");
        for tab in tabs {
            output.push_str("- ");
            output.push_str(&tab_text(tab));
            output.push('\n');
        }
        return Some(output);
    }
    None
}

fn render_snapshot_diff(fields: &serde_json::Map<String, Value>) -> String {
    let heading = fields
        .get("snapshot_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(|| "snapshot diff:".to_owned(), |id| format!("{id} diff:"));
    let mut output = format!("{heading}\n");
    for (key, label) in [
        ("added", "added"),
        ("removed", "removed"),
        ("changed", "changed"),
    ] {
        let Some(items) = fields
            .get(key)
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
        else {
            continue;
        };
        output.push_str(label);
        output.push_str(":\n");
        for item in items {
            output.push_str("- ");
            output.push_str(&snapshot_item_text(item));
            output.push('\n');
        }
    }
    if let Some(hint) = fields
        .get("hint")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        output.push_str("hint: ");
        output.push_str(hint);
        output.push('\n');
    }
    output
}

fn snapshot_item_text(item: &Value) -> String {
    let Some(fields) = item.as_object() else {
        return human_scalar(item);
    };
    for key in ["ref", "refkey"] {
        if let Some(reference) = fields
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return reference.to_owned();
        }
    }
    let role = fields
        .get("role")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("node");
    match fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        Some(name) => format!("{role} {}", go_quote(name)),
        None => role.to_owned(),
    }
}

fn render_state_payload(fields: &serde_json::Map<String, Value>) -> Option<String> {
    for (field, operation) in [
        ("saved", "saved"),
        ("loaded", "loaded"),
        ("cleared", "cleared"),
    ] {
        if let Some(name) = fields.get(field).and_then(Value::as_str) {
            let mut output = format!("{operation}: {name}\n");
            if operation != "cleared" {
                if let Some(metadata) = fields.get("metadata").and_then(Value::as_object) {
                    output.push_str(&render_state_metadata(metadata, "metadata:"));
                }
            }
            return Some(output);
        }
    }
    for field in ["removed", "states"] {
        if let Some(values) = fields
            .get(field)
            .and_then(Value::as_array)
            .filter(|values| values.iter().all(Value::is_string))
        {
            let mut output = format!("{field}:\n");
            for value in values {
                output.push_str("- ");
                output.push_str(value.as_str().expect("validated string"));
                output.push('\n');
            }
            return Some(output);
        }
    }
    if let Some(metadata) = fields.get("metadata").and_then(Value::as_object) {
        return Some(render_state_metadata(metadata, "metadata:"));
    }
    if ["schema_version", "saved_at", "expires_at", "key_source"]
        .iter()
        .any(|k| fields.contains_key(*k))
        && fields.contains_key("origins")
    {
        return Some(render_state_metadata(fields, ""));
    }
    None
}

fn render_state_metadata(fields: &serde_json::Map<String, Value>, heading: &str) -> String {
    let mut output = String::new();
    if !heading.is_empty() {
        output.push_str(heading);
        output.push('\n');
    }
    for (key, label) in [
        ("name", "name"),
        ("schema_version", "schema version"),
        ("saved_at", "saved at"),
        ("expires_at", "expires at"),
        ("key_source", "key source"),
    ] {
        if let Some(value) = fields.get(key) {
            output.push_str(label);
            output.push_str(": ");
            output.push_str(&human_scalar(value));
            output.push('\n');
        }
    }
    if let Some(origins) = fields.get("origins").and_then(Value::as_array) {
        output.push_str("origins:\n");
        for origin in origins {
            let fields = origin.as_object();
            let name = fields
                .and_then(|v| v.get("origin"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let cookies = fields
                .and_then(|v| v.get("cookie_count"))
                .and_then(json_integer)
                .unwrap_or(0);
            let local = fields
                .and_then(|v| v.get("local_storage_keys"))
                .and_then(json_integer)
                .unwrap_or(0);
            let session = fields
                .and_then(|v| v.get("session_storage_keys"))
                .and_then(json_integer)
                .unwrap_or(0);
            output.push_str(&format!(
                "- {name} (cookies={cookies}, local={local}, session={session})\n"
            ));
        }
    }
    output
}

fn cookie_text(cookie: &Value) -> String {
    let Some(fields) = cookie.as_object() else {
        return human_scalar(cookie);
    };
    let mut parts = vec![fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("cookie")
        .to_owned()];
    for (key, label) in [
        ("domain", "domain"),
        ("path", "path"),
        ("same_site", "same_site"),
    ] {
        if let Some(value) = fields
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
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

fn tab_text(tab: &Value) -> String {
    let Some(fields) = tab.as_object() else {
        return human_scalar(tab);
    };
    let id = fields.get("id").and_then(Value::as_str).unwrap_or("");
    let label = fields.get("label").and_then(Value::as_str).unwrap_or("");
    let url = fields.get("url").and_then(Value::as_str).unwrap_or("");
    let mut parts = Vec::new();
    if !id.is_empty() {
        parts.push(id.to_owned());
    }
    if !label.is_empty() && label != id {
        parts.push(go_quote(label));
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

/// Quotes text using Go's `strconv.Quote` escapes and Unicode printability.
fn go_quote(text: &str) -> String {
    use std::fmt::Write as _;

    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\x07' => quoted.push_str("\\a"),
            '\x08' => quoted.push_str("\\b"),
            '\x0c' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\x0b' => quoted.push_str("\\v"),
            character if character < ' ' || character == '\x7f' => {
                write!(quoted, "\\x{:02x}", u32::from(character))
                    .expect("writing into a String cannot fail");
            }
            character if !go_is_print(character) => {
                let scalar = u32::from(character);
                if scalar <= 0xffff {
                    write!(quoted, "\\u{scalar:04x}").expect("writing into a String cannot fail");
                } else {
                    write!(quoted, "\\U{scalar:08x}").expect("writing into a String cannot fail");
                }
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// Go's `unicode.IsPrint` definition: L, M, N, P, and S categories plus ASCII
/// space. ICU4X's locked Unicode 17 data matches the Go 1.27 Unicode tables.
fn go_is_print(character: char) -> bool {
    if character == ' ' {
        return true;
    }
    matches!(
        CodePointMapData::<GeneralCategory>::new().get(character),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::NonspacingMark
            | GeneralCategory::EnclosingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
            | GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

fn human_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => {
            json_integer(value).map_or_else(|| number.to_string(), |integer| integer.to_string())
        }
        Value::Bool(boolean) => boolean.to_string(),
        Value::Null => "<nil>".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn json_integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        let number = value.as_f64()?;
        (number.is_finite()
            && number.fract() == 0.0
            && number >= i64::MIN as f64
            && number < -(i64::MIN as f64))
            .then_some(number as i64)
    })
}

struct TruncationMarker<'a> {
    head: &'a str,
    foot: &'a str,
    hint: &'a str,
    tokens_returned: u64,
    tokens_total: u64,
}

fn truncation_marker(value: &Value) -> Option<TruncationMarker<'_>> {
    let fields = value.as_object()?;
    if !fields.get("truncated")?.as_bool()? {
        return None;
    }
    Some(TruncationMarker {
        head: fields
            .get("head")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        foot: fields
            .get("foot")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        hint: fields
            .get("hint")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        tokens_returned: fields
            .get("tokens_returned")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        tokens_total: fields
            .get("tokens_total")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn human(data: Value) -> String {
        Envelope::ok(data, Vec::new()).render(Format::Text).unwrap()
    }

    #[test]
    fn dedicated_human_renderers_match_go_bytes() {
        let cases = [
            (
                json!({"action":"open","url":"https://example.com/","http_status":200.0}),
                "open https://example.com/ (HTTP 200)\n",
            ),
            (
                json!({"snapshot_id":"snap-1","tree":"- document \"Example\"\n  - heading","refs":{}}),
                "snap-1 tree:\n- document \"Example\"\n  - heading\n",
            ),
            (
                json!({"snapshot_id":"snap-2","added":[{"role":"button","name":"Save"}],"changed":[{"ref":"@e2"}],"hint":"refresh"}),
                "snap-2 diff:\nadded:\n- button \"Save\"\nchanged:\n- @e2\nhint: refresh\n",
            ),
            (
                json!({"saved":"demo","metadata":{"name":"demo","schema_version":3.0,"key_source":"environment","origins":[{"origin":"https://example.com","cookie_count":1.0,"local_storage_keys":2.0,"session_storage_keys":0.0}]}}),
                "saved: demo\nmetadata:\nname: demo\nschema version: 3\nkey source: environment\norigins:\n- https://example.com (cookies=1, local=2, session=0)\n",
            ),
            (
                json!({"origin":"https://example.com","cookies":[{"name":"session","value":"secret","domain":".example.com","path":"/","secure":true,"http_only":true}]}),
                "origin: https://example.com\ncookies:\n- session domain=.example.com path=/ secure httpOnly\n",
            ),
            (
                json!({"active":"t1","tabs":[{"id":"t1","label":"research","url":"https://example.com","active":true},{"id":"t2","url":"about:blank","active":false}]}),
                "active: t1\ntabs:\n- t1 \"research\" https://example.com [active]\n- t2 about:blank\n",
            ),
        ];
        for (data, expected) in cases {
            assert_eq!(human(data), expected);
        }
    }

    #[test]
    fn human_cookie_renderer_does_not_expose_values() {
        let rendered = human(json!({"cookies":[{"name":"auth","value":"secret-token"}]}));
        assert_eq!(rendered, "cookies:\n- auth\n");
        assert!(!rendered.contains("secret-token"));
    }

    #[test]
    fn human_tab_and_snapshot_names_use_go_control_escapes() {
        let tab = human(json!({"tabs":[{"id":"t1","label":"a\u{1}\t\u{7f}\u{85}b"}]}));
        assert_eq!(tab, "tabs:\n- t1 \"a\\x01\\t\\x7f\\u0085b\"\n");

        let snapshot = human(json!({"added":[{"role":"button","name":"a\u{1}\nb"}]}));
        assert_eq!(
            snapshot,
            "snapshot diff:\nadded:\n- button \"a\\x01\\nb\"\n"
        );
    }

    #[test]
    fn human_names_escape_go_nonprintable_unicode_categories() {
        let rendered = human(
            json!({"tabs":[{"id":"t1","label":"\u{200b}\u{2028}\u{e000}\u{0378}\u{00a0}\u{00ad}\u{10ffff}"}]}),
        );
        assert_eq!(
            rendered,
            "tabs:\n- t1 \"\\u200b\\u2028\\ue000\\u0378\\u00a0\\u00ad\\U0010ffff\"\n"
        );
    }
}
