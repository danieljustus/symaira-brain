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
            out.push_str(&crate::go_value::marshal_plain(&Value::String(
                error.message.clone(),
            )));
            if !error.hint.is_empty() {
                out.push_str(",\"hint\":");
                out.push_str(&crate::go_value::marshal_plain(&Value::String(
                    error.hint.clone(),
                )));
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
        output.push_str("    - ");
        write_yaml_field_at(
            output,
            "kind",
            &Value::String(warning.kind.clone()),
            6,
            false,
        );
        for (key, value) in [
            ("severity", warning.severity.as_str()),
            ("message", warning.message.as_str()),
            ("ref", warning.r#ref.as_str()),
            ("excerpt", warning.excerpt.as_str()),
        ] {
            if !value.is_empty() {
                write_yaml_field(output, key, &Value::String(value.to_owned()), 6);
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

/// yaml.v3 `increase_indent` below a block mapping key: the next multiple of
/// the 4-column indent (a key in a sequence item sits at an odd offset).
fn below_key(indent: usize) -> usize {
    4 * ((indent + 4) / 4)
}

/// Writes `key: value` for a block mapping whose keys sit in column `indent`.
pub(crate) fn write_yaml_field(output: &mut String, key: &str, value: &Value, indent: usize) {
    write_yaml_field_at(output, key, value, indent, true);
}

/// Like [`write_yaml_field`]; `pad` is false for the first key of a mapping
/// that follows `- ` on the same line.
pub(crate) fn write_yaml_field_at(
    output: &mut String,
    key: &str,
    value: &Value,
    indent: usize,
    pad: bool,
) {
    if pad {
        output.push_str(&" ".repeat(indent));
    }
    let name = crate::go_value::yaml_name(key);
    // yaml.v3 `check_simple_key`: a multi-line or over-long key is written as
    // an explicit `? key` / `: value` pair.
    if name.len() > 128 || name.chars().any(is_yaml_break) {
        output.push_str("? ");
        match crate::yaml_str::encode(name, false) {
            crate::yaml_str::Encoded::Literal { header } => output.push_str(
                &crate::yaml_str::literal_text(&header, name, &" ".repeat(below_key(indent))),
            ),
            crate::yaml_str::Encoded::Inline(text) => {
                output.push_str(&text);
                output.push('\n');
            }
        }
        output.push_str(&" ".repeat(indent));
        output.push(':');
    } else {
        output.push_str(&yaml_key(key));
        output.push(':');
    }
    write_yaml_value(output, value, indent, false);
}

fn is_yaml_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// yaml.v3 `keyList.Less` for string keys: letters compare by code point,
/// a letter sorts after a non-letter (unless the common prefix ended in a
/// digit), and digit runs compare by numeric value.
fn yaml_key_less(a: &str, b: &str) -> bool {
    let ar: Vec<char> = a.chars().collect();
    let br: Vec<char> = b.chars().collect();
    let mut digits = false;
    for i in 0..ar.len().min(br.len()) {
        if ar[i] == br[i] {
            digits = ar[i].is_numeric();
            continue;
        }
        let (al, bl) = (ar[i].is_alphabetic(), br[i].is_alphabetic());
        if al && bl {
            return ar[i] < br[i];
        }
        if al || bl {
            return if digits { al } else { bl };
        }
        let (mut an, mut bn) = (0i64, 0i64);
        if ar[i] == '0' || br[i] == '0' {
            for j in (0..i).rev() {
                if !ar[j].is_numeric() {
                    break;
                }
                if ar[j] != '0' {
                    an = 1;
                    bn = 1;
                    break;
                }
            }
        }
        let mut ai = i;
        while ai < ar.len() && ar[ai].is_numeric() {
            an = an
                .wrapping_mul(10)
                .wrapping_add(i64::from(ar[ai] as u32) - i64::from('0' as u32));
            ai += 1;
        }
        let mut bi = i;
        while bi < br.len() && br[bi].is_numeric() {
            bn = bn
                .wrapping_mul(10)
                .wrapping_add(i64::from(br[bi] as u32) - i64::from('0' as u32));
            bi += 1;
        }
        if an != bn {
            return an < bn;
        }
        if ai != bi {
            return ai < bi;
        }
        return ar[i] < br[i];
    }
    ar.len() < br.len()
}

/// Entries in yaml.v3 order. Struct-shaped objects carry ordered keys that
/// already encode the declaration order and are left alone.
fn yaml_entries(values: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = values.iter().collect();
    if entries.iter().any(|(key, _)| key.starts_with('\u{1}')) {
        return entries;
    }
    // Stable insertion sort: `yaml_key_less` is not a total order, so the
    // standard sorts (which may reject that) are not used.
    for index in 1..entries.len() {
        let mut position = index;
        while position > 0 && yaml_key_less(entries[position].0, entries[position - 1].0) {
            entries.swap(position, position - 1);
            position -= 1;
        }
    }
    entries
}

/// Writes `- value` for a block sequence whose dashes sit in column `indent`;
/// `pad` is false for the first item of a sequence that follows `- ` inline.
fn write_yaml_sequence_item_at(output: &mut String, value: &Value, indent: usize, pad: bool) {
    if pad {
        output.push_str(&" ".repeat(indent));
    }
    output.push('-');
    write_yaml_value(output, value, indent, true);
}

/// Writes the value after `key:` (`in_seq` false, `indent` = key column) or
/// after `-` (`in_seq` true, `indent` = dash column), following the layout of
/// yaml.v3's emitter: collections below a key start at the next 4-column
/// stop, the first node of a sequence item starts two columns right of the
/// dash on the same line.
pub(crate) fn write_yaml_value(output: &mut String, value: &Value, indent: usize, in_seq: bool) {
    let child = if in_seq {
        indent + 2
    } else {
        below_key(indent)
    };
    let opener = if in_seq { ' ' } else { '\n' };
    match value {
        Value::Object(values) if !values.is_empty() => {
            output.push(opener);
            for (index, (key, child_value)) in yaml_entries(values).into_iter().enumerate() {
                write_yaml_field_at(output, key, child_value, child, !(in_seq && index == 0));
            }
        }
        Value::Array(values) if !values.is_empty() => {
            output.push(opener);
            for (index, item) in values.iter().enumerate() {
                write_yaml_sequence_item_at(output, item, child, !(in_seq && index == 0));
            }
        }
        Value::Object(_) => output.push_str(" {}\n"),
        Value::Array(_) => output.push_str(" []\n"),
        Value::String(text) if text.contains('\n') => match crate::yaml_str::encode(text, false) {
            crate::yaml_str::Encoded::Literal { header } => {
                output.push(' ');
                output.push_str(&crate::yaml_str::literal_text(
                    &header,
                    text,
                    &" ".repeat(child),
                ));
            }
            crate::yaml_str::Encoded::Inline(text) => {
                output.push(' ');
                output.push_str(&text);
                output.push('\n');
            }
        },
        _ => {
            output.push(' ');
            let scalar = yaml_scalar(value);
            match value {
                Value::String(text)
                    if scalar.starts_with('\'') && text.contains(['\u{2028}', '\u{2029}']) =>
                {
                    output.push_str(&crate::yaml_str::single_quoted_breaks(
                        &scalar,
                        &" ".repeat(child),
                    ));
                }
                _ => output.push_str(&scalar),
            }
            output.push('\n');
        }
    }
}

fn yaml_key(value: &str) -> String {
    crate::yaml_str::inline(crate::go_value::yaml_name(value), true)
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
        Value::String(value) => {
            crate::go_value::raw_text(value).map_or_else(|| yaml_string(value), str::to_owned)
        }
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
