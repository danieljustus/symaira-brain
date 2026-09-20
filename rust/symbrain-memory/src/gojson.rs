//! Go-compatible JSON rendering for the memory command output.
//!
//! The shipped CLI encodes its rows with `encoding/json`: compact, omitting
//! empty fields (`omitempty`), rendering `time.Time` as RFC3339 with trailing
//! zeros trimmed and a `float64` in its shortest form (`0`, not `0.0`).
//! `serde_json` would print `0.0`, so the rows below render themselves.

use chrono::{DateTime, Utc};

/// Renders a string as a JSON string literal.
pub(crate) fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Renders a float the way Go's encoder does: shortest round-trip form
/// without a forced fraction.
pub(crate) fn number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let mut rendered = format!("{value}");
    if let Some(stripped) = rendered.strip_suffix(".0") {
        rendered = stripped.to_owned();
    }
    rendered
}

/// Renders a `float32` the way Go's `encoding/json` does.
///
/// Go switches to exponent notation when the magnitude is below `1e-6` or at
/// least `1e21` (compared as `float32`), pads the exponent to two digits and
/// keeps the shortest round-trip mantissa; Rust's `Display` never uses an
/// exponent, so that branch is spelled out here.
pub(crate) fn number_f32(value: f32) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    if magnitude < 1e-6 || magnitude >= 1e21 {
        return exponent_form(f64::from(value));
    }
    let mut rendered = format!("{value}");
    if let Some(stripped) = rendered.strip_suffix(".0") {
        rendered = stripped.to_owned();
    }
    rendered
}

/// Renders `value` in Go's `strconv` `'e'` form (`1e-07`, not `1e-7`).
fn exponent_form(value: f64) -> String {
    let rendered = format!("{value:e}");
    let Some((mantissa, exponent)) = rendered.split_once('e') else {
        return rendered;
    };
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ('-', digits),
        None => ('+', exponent.strip_prefix('+').unwrap_or(exponent)),
    };
    let padded = if digits.len() < 2 {
        format!("0{digits}")
    } else {
        digits.to_owned()
    };
    format!("{mantissa}e{sign}{padded}")
}

/// Renders a stored JSON value the way Go's encoder would.
pub(crate) fn render_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => quote(text),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_owned(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_owned()),
    }
}

/// Renders an optional timestamp the way Go marshals `time.Time`.
pub(crate) fn timestamp(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(|time| {
        let base = time.format("%Y-%m-%dT%H:%M:%S").to_string();
        let nanos = time.timestamp_subsec_nanos();
        if nanos == 0 {
            return format!("{base}Z");
        }
        let fraction = format!("{nanos:09}");
        format!("{base}.{}Z", fraction.trim_end_matches('0'))
    })
}

/// Accumulates JSON object members in declaration order.
#[derive(Default)]
pub(crate) struct Members {
    rendered: Vec<String>,
}

impl Members {
    /// Adds a required string member.
    pub(crate) fn string(&mut self, name: &str, value: &str) {
        self.rendered
            .push(format!("{}:{}", quote(name), quote(value)));
    }

    /// Adds a string member unless it is empty.
    pub(crate) fn string_if_present(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.string(name, value);
        }
    }

    /// Adds a required number member.
    pub(crate) fn number(&mut self, name: &str, value: f64) {
        self.rendered
            .push(format!("{}:{}", quote(name), number(value)));
    }

    /// Adds an integer member.
    pub(crate) fn integer(&mut self, name: &str, value: i64) {
        self.rendered.push(format!("{}:{value}", quote(name)));
    }

    /// Adds a timestamp member unless it is absent.
    pub(crate) fn time_if_present(&mut self, name: &str, value: Option<DateTime<Utc>>) {
        if let Some(rendered) = timestamp(value) {
            self.rendered
                .push(format!("{}:{}", quote(name), quote(&rendered)));
        }
    }

    /// Adds an optional timestamp, which the shipped struct declares as
    /// `omitempty` but never omits when the driver returns a value.
    pub(crate) fn time(&mut self, name: &str, value: Option<DateTime<Utc>>) {
        match timestamp(value) {
            Some(rendered) => self
                .rendered
                .push(format!("{}:{}", quote(name), quote(&rendered))),
            None => self.rendered.push(format!("{}:null", quote(name))),
        }
    }

    /// Adds raw, already rendered JSON.
    pub(crate) fn raw(&mut self, name: &str, rendered: &str) {
        self.rendered.push(format!("{}:{rendered}", quote(name)));
    }

    /// Finishes the object.
    pub(crate) fn finish(self) -> String {
        format!("{{{}}}", self.rendered.join(","))
    }
}
