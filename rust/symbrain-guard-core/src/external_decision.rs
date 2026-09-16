//! Bounded, side-effect-free external guard decision evaluation.
//!
//! This is deliberately separate from the catalog/model contract: its wire
//! object is the flat `guard decide` request used by external callers.

use std::fmt;
use std::net::IpAddr;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

#[path = "external_decode.rs"]
mod decoder;
use decoder::{decode_request, warnings_empty};
#[path = "external_json.rs"]
mod json_syntax;

/// Maximum request size accepted by the external decision contract.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Flat request accepted by the external decision contract.
#[derive(Debug, Clone, Deserialize)]
pub struct ExternalDecisionRequest {
    #[serde(default, deserialize_with = "deserialize_zero_string")]
    pub command: String,
    #[serde(default, deserialize_with = "deserialize_zero_string")]
    pub risk_class: String,
    #[serde(default, deserialize_with = "deserialize_zero_string")]
    pub domain: String,
    #[serde(default, deserialize_with = "deserialize_warnings")]
    pub warnings: Option<Vec<String>>,
    #[serde(default)]
    pub deadline: Option<DateTime<FixedOffset>>,
}

/// The only decisions produced by this contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalDecision {
    Allow,
    Confirm,
    Deny,
}

/// The external response. CLI serialization belongs to a later adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalDecisionResponse {
    pub decision: ExternalDecision,
    pub reason: String,
}

/// Audit record passed to the injected sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalDecisionAudit {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub risk_class: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub domain: String,
    #[serde(skip_serializing_if = "warnings_empty")]
    pub warnings: Option<Vec<String>>,
    pub decision: ExternalDecision,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub decided_at: String,
}

/// Error reported by an audit sink.
pub trait AuditSink {
    /// Persist one decision record. A failure is security-relevant.
    ///
    /// # Errors
    ///
    /// Returns the sink's persistence diagnostic when the record cannot be
    /// written.
    fn write(&mut self, record: &ExternalDecisionAudit) -> Result<(), String>;
}

impl<F> AuditSink for F
where
    F: FnMut(&ExternalDecisionAudit) -> Result<(), String>,
{
    fn write(&mut self, record: &ExternalDecisionAudit) -> Result<(), String> {
        self(record)
    }
}

/// Evaluate one bounded JSON request at an injected instant and audit it.
///
/// JSON decoding follows Go's `encoding/json` behavior for this contract:
/// unknown object fields are ignored, omitted scalar fields use their zero
/// values, and malformed values fail closed. Every outcome is sent to `sink`.
/// An audit failure changes only an allow/confirm outcome to deny.
pub fn evaluate_at(
    input: &[u8],
    now: DateTime<FixedOffset>,
    sink: &mut dyn AuditSink,
) -> ExternalDecisionResponse {
    let (request, response) = evaluate_without_audit(input, now);
    finish_audit(&request, response, now, sink)
}

/// Audit a request-read failure using the same diagnostic text as Go.
///
/// This exists for the adapter boundary, where reading stdin can fail before
/// any request bytes are available.
pub fn evaluate_read_error_at(
    error: impl fmt::Display,
    now: DateTime<FixedOffset>,
    sink: &mut dyn AuditSink,
) -> ExternalDecisionResponse {
    let response = deny(format!("decide: read request: {error}"));
    let request = empty_request();
    finish_audit(&request, response, now, sink)
}

fn finish_audit(
    request: &ExternalDecisionRequest,
    mut response: ExternalDecisionResponse,
    now: DateTime<FixedOffset>,
    sink: &mut dyn AuditSink,
) -> ExternalDecisionResponse {
    let record = audit_record(request, &response, now);
    if let (Err(error), ExternalDecision::Allow | ExternalDecision::Confirm) =
        (sink.write(&record), response.decision)
    {
        response = ExternalDecisionResponse {
            decision: ExternalDecision::Deny,
            reason: format!("audit: write decision record: {error}"),
        };
    }
    response
}

fn evaluate_without_audit(
    input: &[u8],
    now: DateTime<FixedOffset>,
) -> (ExternalDecisionRequest, ExternalDecisionResponse) {
    if input.len() > MAX_REQUEST_BYTES {
        return fail_closed("decide: request exceeds maximum size of 65536 bytes");
    }
    if std::str::from_utf8(input).is_ok_and(|text| text.trim().is_empty()) {
        return fail_closed("decide: empty request");
    }

    if let Err(error) = json_syntax::validate(input) {
        return fail_closed(&format!("decide: parse request: {error}"));
    }
    let decoded = match decode_request(input) {
        Ok(decoded) => decoded,
        Err(error) => {
            return fail_closed(&format!(
                "decide: parse request: {}",
                parse_error(&error, input)
            ));
        }
    };
    if let Some(error) = decoded.type_error {
        return fail_closed(&format!("decide: parse request: {error}"));
    }
    let request = decoded.request;
    if request.command.is_empty() {
        return (request, deny("decide: missing command"));
    }
    if request.deadline.is_some_and(|deadline| {
        let zero = chrono::NaiveDate::from_ymd_opt(1, 1, 1)
            .expect("Go zero date")
            .and_hms_opt(0, 0, 0)
            .expect("Go zero time");
        deadline.naive_utc() != zero && now >= deadline
    }) {
        return (request, deny("decide: request deadline expired"));
    }

    let risk = request.risk_class.trim().to_ascii_lowercase();
    let warnings = clean_warnings(request.warnings.as_deref().unwrap_or_default());
    let domain = request.domain.trim().to_ascii_lowercase();
    let response = match risk.as_str() {
        "low" | "medium" if warnings.is_empty() => allow(format!("{risk} risk class, no warnings")),
        "low" | "medium" => confirm(format!(
            "{risk} risk class with warnings: {}",
            warnings.join("; ")
        )),
        "high" if warnings.is_empty() => confirm("high risk class requires confirmation"),
        "high" => deny(format!(
            "high risk class with warnings: {}",
            warnings.join("; ")
        )),
        "critical" if warnings.is_empty() && is_loopback(&domain) => allow(format!(
            "critical risk class on allowlisted domain {}, no warnings",
            symbrain_core::config::format_go_quoted(request.domain.as_ref())
        )),
        "critical" => deny("critical risk class: requires allowlisted domain and no warnings"),
        _ => deny(format!(
            "decide: unknown risk class {}",
            symbrain_core::config::format_go_quoted(request.risk_class.as_ref())
        )),
    };
    (request, response)
}

fn parse_error(error: &serde_json::Error, input: &[u8]) -> String {
    let message = error.to_string();
    if let (true, Some(&byte)) = (
        message.starts_with("trailing characters"),
        trailing_byte(input, error.line(), error.column()),
    ) {
        return format!(
            "invalid character {} after top-level value",
            quote_go_char(byte)
        );
    }
    if message.starts_with("EOF while parsing") {
        "unexpected end of JSON input".to_owned()
    } else {
        message
    }
}

fn trailing_byte(input: &[u8], line: usize, column: usize) -> Option<&u8> {
    if line == 0 || column == 0 {
        return None;
    }
    let line_start = if line == 1 {
        0
    } else {
        input
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .nth(line - 2)
            .and_then(|(index, _)| index.checked_add(1))?
    };
    line_start
        .checked_add(column - 1)
        .and_then(|index| input.get(index))
}

fn quote_go_char(byte: u8) -> String {
    let escaped = match byte {
        b'\'' => "\\'".to_owned(),
        b'"' => "\"".to_owned(),
        b'\\' => "\\\\".to_owned(),
        0x07 => "\\a".to_owned(),
        0x08 => "\\b".to_owned(),
        0x0c => "\\f".to_owned(),
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        b'\t' => "\\t".to_owned(),
        0x0b => "\\v".to_owned(),
        // Go's quoteChar calls strconv.Quote(string(c)); IsPrint excludes
        // Latin-1 spacing U+00A0 and soft hyphen U+00AD.
        0x20..=0x7e | 0xa1..=0xac | 0xae..=0xff => char::from(byte).to_string(),
        0x80..=0xa0 | 0xad => format!("\\u{byte:04x}"),
        _ => format!("\\x{byte:02x}"),
    };
    format!("'{escaped}'")
}

fn clean_warnings(warnings: &[String]) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| warning.trim())
        .filter(|warning| !warning.is_empty())
        .map(str::to_owned)
        .collect()
}

fn deserialize_zero_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_warnings<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<String>>::deserialize(deserializer)
}

fn is_loopback(domain: &str) -> bool {
    if let Some((address, zone)) = domain.split_once('%') {
        return !zone.is_empty()
            && address
                .parse::<std::net::Ipv6Addr>()
                .is_ok_and(ipv6_loopback);
    }
    domain.parse::<IpAddr>().is_ok_and(|address| match address {
        IpAddr::V4(address) => address.is_loopback(),
        IpAddr::V6(address) => ipv6_loopback(address),
    })
}

fn ipv6_loopback(address: std::net::Ipv6Addr) -> bool {
    address.is_loopback() || address.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
}

fn audit_record(
    request: &ExternalDecisionRequest,
    response: &ExternalDecisionResponse,
    now: DateTime<FixedOffset>,
) -> ExternalDecisionAudit {
    ExternalDecisionAudit {
        id: format!(
            "evt_1_decide_{}",
            now.timestamp_nanos_opt().unwrap_or_default()
        ),
        command: request.command.clone(),
        risk_class: request.risk_class.clone(),
        domain: request.domain.clone(),
        warnings: request.warnings.clone(),
        decision: response.decision,
        reason: response.reason.clone(),
        decided_at: crate::go_json::format_go_rfc3339(now),
    }
}

fn fail_closed(reason: &str) -> (ExternalDecisionRequest, ExternalDecisionResponse) {
    (empty_request(), deny(reason))
}

fn empty_request() -> ExternalDecisionRequest {
    ExternalDecisionRequest {
        command: String::new(),
        risk_class: String::new(),
        domain: String::new(),
        warnings: None,
        deadline: None,
    }
}

fn allow(reason: impl Into<String>) -> ExternalDecisionResponse {
    response(ExternalDecision::Allow, reason)
}

fn confirm(reason: impl Into<String>) -> ExternalDecisionResponse {
    response(ExternalDecision::Confirm, reason)
}

fn deny(reason: impl Into<String>) -> ExternalDecisionResponse {
    response(ExternalDecision::Deny, reason)
}

fn response(decision: ExternalDecision, reason: impl Into<String>) -> ExternalDecisionResponse {
    ExternalDecisionResponse {
        decision,
        reason: reason.into(),
    }
}

impl fmt::Display for ExternalDecision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Allow => "allow",
            Self::Confirm => "confirm",
            Self::Deny => "deny",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::quote_go_char;

    #[test]
    fn quote_go_char_preserves_frozen_latin1_printability() {
        for byte in 0x80..=0xa0 {
            assert_eq!(quote_go_char(byte), format!("'\\u{byte:04x}'"));
        }
        assert_eq!(quote_go_char(0xa1), "'¡'");
        assert_eq!(quote_go_char(0xad), "'\\u00ad'");
        assert_eq!(quote_go_char(0xae), "'®'");
    }
}
