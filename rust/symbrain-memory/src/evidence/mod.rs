//! Brain-owned grounded evidence, compatible with pinned Go evidencekit.
//!
//! Spans use UTF-8 byte offsets, even though their JSON keys say `char_*`.
//! Fuzzy alignment is diagnostic only; persistence uses strict validation.

mod fuzzy;

use std::collections::BTreeMap;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

/// Identity of the document, session or activity segment containing evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceRef {
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
}

/// Half-open UTF-8 byte range in the original source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Span {
    #[serde(rename = "char_start")]
    pub start: i64,
    #[serde(rename = "char_end")]
    pub end: i64,
}

/// A fact and the source fragment that justifies it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extraction {
    pub source: SourceRef,
    pub text: String,
    pub evidence_text: String,
    pub span: Span,
    pub alignment_status: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

/// Typed validation categories preserve Go's sentinel error contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    Unmatched,
    EmptyEvidence,
    InvalidSpan(Span),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unmatched => f.write_str("evidencekit: extraction is unmatched, not grounded"),
            Self::EmptyEvidence => f.write_str("evidencekit: empty evidence text"),
            Self::InvalidSpan(span) => {
                write!(f, "evidencekit: invalid span [{},{})", span.start, span.end)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl Extraction {
    /// Checks grounded-only ingestion, rejecting fuzzy alignment by default.
    ///
    /// # Errors
    /// Returns the first Go-compatible validation category.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_with_options(false)
    }

    /// Validates with an explicit opt-in for approximate alignment.
    ///
    /// # Errors
    /// Returns unmatched, empty-evidence or invalid-span in that order.
    pub fn validate_with_options(&self, accept_fuzzy: bool) -> Result<(), ValidationError> {
        if self.alignment_status == "unmatched"
            || (self.alignment_status == "fuzzy" && !accept_fuzzy)
        {
            return Err(ValidationError::Unmatched);
        }
        if self.evidence_text.is_empty() {
            return Err(ValidationError::EmptyEvidence);
        }
        if self.span.start < 0 || self.span.end < self.span.start {
            return Err(ValidationError::InvalidSpan(self.span));
        }
        // Go's validator also accepts zero-length spans and unknown status
        // strings. Do not silently tighten the frozen persistence contract.
        Ok(())
    }
}

/// Finds the first byte-identical fragment.
#[must_use]
pub fn align_exact(source: &str, evidence: &str) -> Option<Span> {
    if evidence.is_empty() {
        return None;
    }
    source
        .find(evidence)
        .and_then(|start| span(start, start + evidence.len()))
}

/// Finds collapsed Unicode whitespace and maps the result to original bytes.
#[must_use]
pub fn align_normalized(source: &str, evidence: &str) -> Option<Span> {
    let (normalized, offsets) = normalize(source);
    let (needle, _) = normalize(evidence);
    let needle = needle.trim_matches(go_space);
    if needle.is_empty() {
        return None;
    }
    let byte_start = normalized.find(needle)?;
    let start = normalized[..byte_start].chars().count();
    let end = start + needle.chars().count();
    span(offsets.get(start)?.0, offsets.get(end.checked_sub(1)?)?.1)
}

/// Tries exact, normalized, then approximate alignment in the frozen order.
#[must_use]
pub fn align(source: &str, evidence: &str) -> (Span, &'static str) {
    if let Some(span) = align_exact(source, evidence) {
        (span, "exact")
    } else if let Some(span) = align_normalized(source, evidence) {
        (span, "normalized")
    } else if let Some(span) = align_fuzzy(source, evidence) {
        (span, "fuzzy")
    } else {
        (Span::default(), "unmatched")
    }
}

pub use fuzzy::align_fuzzy;

/// Writes compact JSONL with Go HTML escaping and sorted attribute keys.
///
/// # Errors
/// Returns a writer error, including a failure after earlier complete lines.
pub fn encode_jsonl(writer: &mut impl Write, extractions: &[Extraction]) -> io::Result<()> {
    for extraction in extractions {
        let json = serde_json::to_string(extraction).map_err(io::Error::other)?;
        let escaped = json
            .replace('&', "\\u0026")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        writeln!(writer, "{escaped}")?;
    }
    Ok(())
}

// Rust's White_Space property matches unicode.IsSpace; spell it explicitly so
// Unicode database upgrades cannot change the immutable Go oracle contract.
fn go_space(ch: char) -> bool {
    matches!(ch, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn normalize(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut normalized = String::new();
    let mut offsets = Vec::new();
    let mut in_space = false;
    for (offset, ch) in text.char_indices() {
        if go_space(ch) {
            if !in_space {
                normalized.push(' ');
                offsets.push((offset, offset + ch.len_utf8()));
                in_space = true;
            }
        } else {
            normalized.push(ch);
            offsets.push((offset, offset + ch.len_utf8()));
            in_space = false;
        }
    }
    (normalized, offsets)
}

fn span(start: usize, end: usize) -> Option<Span> {
    Some(Span {
        start: i64::try_from(start).ok()?,
        end: i64::try_from(end).ok()?,
    })
}
