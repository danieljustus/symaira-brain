#![deny(unsafe_code)]

//! Unified JSON, YAML and human output envelope contracts.

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
                let mut output = self.render_json()?;
                output.push('\n');
                Ok(output)
            }
            Format::Yaml => render_yaml(self),
            Format::Text => Ok(render_human(self)),
        }
    }
}

impl Envelope {
    /// Compact JSON in Go's field order with Go's number/escape rules.
    fn render_json(&self) -> Result<String, RenderError> {
        let mut out = format!("{{\"success\":{}", self.success);
        if !self.data.is_null() {
            out.push_str(",\"data\":");
            out.push_str(&crate::go_value::marshal_plain(&self.data));
        }
        if !self.warnings.is_empty() {
            out.push_str(",\"warnings\":[");
            for (index, warning) in self.warnings.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                let mut fields = vec![("kind", warning.kind.as_str())];
                for (key, value) in [
                    ("severity", warning.severity.as_str()),
                    ("message", warning.message.as_str()),
                    ("ref", warning.r#ref.as_str()),
                    ("excerpt", warning.excerpt.as_str()),
                ] {
                    if key == "message" || !value.is_empty() {
                        fields.push((key, value));
                    }
                }
                out.push_str(&json_object(&fields));
            }
            out.push(']');
        }
        if let Some(error) = &self.error {
            out.push_str(",\"error\":{\"code\":");
            let code = serde_json::to_value(error.code).map_err(RenderError::Json)?;
            out.push_str(&crate::go_value::marshal_plain(&code));
            out.push_str(",\"message\":");
            out.push_str(&crate::go_value::marshal_plain(&Value::String(error.message.clone())));
            if !error.hint.is_empty() {
                out.push_str(",\"hint\":");
                out.push_str(&crate::go_value::marshal_plain(&Value::String(error.hint.clone())));
            }
            if let Some(details) = &error.details
                && !details.as_object().is_some_and(serde_json::Map::is_empty)
            {
                out.push_str(",\"details\":");
                out.push_str(&crate::go_value::marshal_plain(details));
            }
            if let Some(retryable) = error.retryable {
                out.push_str(&format!(",\"retryable\":{retryable}"));
            }
            if let Some(confirm) = error.requires_user_confirmation {
                out.push_str(&format!(",\"requires_user_confirmation\":{confirm}"));
            }
            if !error.resume_hint.is_empty() {
                out.push_str(",\"resume_hint\":");
                out.push_str(&crate::go_value::marshal_plain(&Value::String(
                    error.resume_hint.clone(),
                )));
            }
            out.push('}');
        }
        out.push('}');
        Ok(out)
    }
}

fn json_object(fields: &[(&str, &str)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(key, value)| {
            format!(
                "\"{key}\":{}",
                crate::go_value::marshal_plain(&Value::String((*value).to_owned()))
            )
        })
        .collect();
    format!("{{{}}}", parts.join(","))
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
    write_yaml_field(output, "hint", &Value::String(error.hint.clone()), 4);
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
        Value::String(text) if text.contains('\n') => {
            match crate::yaml_str::encode(text, false) {
                crate::yaml_str::Encoded::Literal { header, lines } => {
                    output.push(' ');
                    output.push_str(&header);
                    output.push('\n');
                    let pad = " ".repeat(indent + 4);
                    for line in lines {
                        if !line.is_empty() {
                            output.push_str(&pad);
                            output.push_str(&line);
                        }
                        output.push('\n');
                    }
                }
                crate::yaml_str::Encoded::Inline(text) => {
                    output.push(' ');
                    output.push_str(&text);
                    output.push('\n');
                }
            }
        }
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
    crate::yaml_str::inline(value, true)
}

pub(crate) fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) if value.is_f64() => {
            let float = value.as_f64().unwrap_or_default();
            if float.is_infinite() {
                if float > 0.0 { ".inf" } else { "-.inf" }.to_owned()
            } else if float.is_nan() {
                ".nan".to_owned()
            } else {
                crate::go_value::float_v(float)
            }
        }
        Value::Number(value) => value.to_string(),
        Value::String(value) => yaml_string(value),
        Value::Array(_) | Value::Object(_) => "null".to_owned(),
    }
}

fn yaml_string(value: &str) -> String {
    crate::yaml_str::inline(value, false)
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
    crate::human::render_value(&envelope.data)
}

struct TruncationMarker<'a> {
    head: &'a str,
    foot: &'a str,
    hint: &'a str,
    tokens_returned: i64,
    tokens_total: i64,
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
            .and_then(Value::as_f64)
            .map_or(0, |v| v as i64),
        tokens_total: fields
            .get("tokens_total")
            .and_then(Value::as_f64)
            .map_or(0, |v| v as i64),
    })
}
