//! Go-compatible rendering of decoded JSON values.
//!
//! The Go CLI receives daemon payloads as `any` (numbers are `float64`, objects
//! are key-sorted maps). These helpers reproduce `fmt.Sprint`, `json.Marshal`
//! and `json.MarshalIndent` over that representation.

use serde_json::Value;

/// `strconv.FormatFloat(f, 'g', -1, 64)` as printed by `fmt` for `%v`.
pub fn float_v(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let sci = format!("{value:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exponent.parse().unwrap_or(0);
    // strconv decides with precision 6 when the digits are shortest.
    if !(-4..6).contains(&exp) {
        return exp_form(mantissa, exp);
    }
    format!("{value}")
}

fn exp_form(mantissa: &str, exp: i32) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exp.abs())
}

/// Number formatting of Go's `encoding/json` for a `float64`.
pub fn float_json(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let abs = value.abs();
    if !(1e-6..1e21).contains(&abs) {
        let sci = format!("{value:e}");
        let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
        let exp: i32 = exponent.parse().unwrap_or(0);
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{}", exp.abs());
    }
    format!("{value}")
}

fn as_f64(number: &serde_json::Number) -> f64 {
    number.as_f64().unwrap_or(0.0)
}

/// `fmt.Sprint(value)` / `fmt.Println(value)` for a decoded JSON value.
pub fn sprint(value: &Value) -> String {
    match value {
        Value::Null => "<nil>".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) if n.is_f64() => float_v(as_f64(n)),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(sprint).collect();
            format!("[{}]", parts.join(" "))
        }
        Value::Object(map) => {
            let mut parts = Vec::new();
            for (key, item) in map {
                parts.push(format!("{key}:{}", sprint(item)));
            }
            format!("map[{}]", parts.join(" "))
        }
    }
}

fn escape(out: &mut String, text: &str, html: bool) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' if html => out.push_str(&format!("\\u{:04x}", c as u32)),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_value(out: &mut String, value: &Value, html: bool, indent: Option<usize>) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if n.is_f64() => out.push_str(&float_json(as_f64(n))),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => escape(out, s, html),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent.map(|n| n + 1));
                write_value(out, item, html, indent.map(|n| n + 1));
            }
            newline(out, indent);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent.map(|n| n + 1));
                escape(out, key, html);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, item, html, indent.map(|n| n + 1));
            }
            newline(out, indent);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>) {
    if let Some(level) = indent {
        out.push('\n');
        for _ in 0..level {
            out.push_str("  ");
        }
    }
}

/// `json.Marshal` with HTML escaping (the library default).
pub fn marshal(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, true, None);
    out
}

/// `encoder.Encode` with `SetEscapeHTML(false)` (without the newline).
pub fn marshal_plain(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, false, None);
    out
}

/// `json.MarshalIndent(value, "", "  ")`.
pub fn marshal_indent(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, true, Some(0));
    out
}

/// `json.MarshalIndent(value, "", "  ")` without HTML escaping.
pub fn marshal_indent_plain(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, false, Some(0));
    out
}

/// Converts every number to `f64`, the representation Go's `any` decoding
/// produces. Typed Go integers (which keep integer formatting) are built by
/// the CLI after this conversion and therefore stay integral.
pub fn to_go_any(value: &Value) -> Value {
    match value {
        Value::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| value.clone(), Value::Number),
        Value::Array(items) => Value::Array(items.iter().map(to_go_any).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), to_go_any(v))).collect()),
        other => other.clone(),
    }
}
