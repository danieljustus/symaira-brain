//! Flat marker regions and target-specific term resolution.

use std::collections::{BTreeMap, BTreeSet};

#[path = "variant_types.rs"]
mod variant_types;
pub use variant_types::*;

#[derive(Clone)]
struct Segment {
    region: bool,
    kind: Kind,
    id: String,
    targets: Vec<String>,
    line: usize,
    lines: Vec<String>,
}
fn problem(code: &str, message: String, line: usize) -> Problem {
    Problem {
        code: code.into(),
        severity: SEVERITY_ERROR.into(),
        message,
        line,
    }
}
fn block_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}
fn term_name(value: &str) -> bool {
    let mut previous_separator = false;
    let mut has_segment = false;
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            has_segment = true;
            previous_separator = false;
            continue;
        }
        if matches!(character, '_' | '-')
            && index > 0
            && !previous_separator
            && value
                .chars()
                .nth(index + 1)
                .is_some_and(|next| next.is_ascii_lowercase() || next.is_ascii_digit())
        {
            previous_separator = true;
            continue;
        }
        return false;
    }
    has_segment && !previous_separator
}
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Block => "block",
        Kind::Only => "only",
        Kind::Except => "except",
    }
}

fn opening(line: &str) -> Option<(Kind, String, Vec<String>)> {
    let text = line.trim();
    let close = text.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = close.strip_prefix("symskills:")?.trim_start();
    if let Some(arguments) = rest.strip_prefix("block") {
        if !arguments.starts_with(char::is_whitespace) {
            return None;
        }
        let mut parts = arguments.split_whitespace();
        let id = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        return Some((Kind::Block, id.into(), Vec::new()));
    }
    for (prefix, kind) in [("only", Kind::Only), ("except", Kind::Except)] {
        if let Some(targets) = rest.strip_prefix(prefix) {
            if !targets.starts_with(char::is_whitespace) {
                continue;
            }
            return Some((
                kind,
                String::new(),
                targets
                    .split(',')
                    .map(str::trim)
                    .filter(|target| !target.is_empty())
                    .map(ToString::to_string)
                    .collect(),
            ));
        }
    }
    None
}
fn closing(line: &str) -> Option<Kind> {
    let text = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    match text {
        "/symskills:block" => Some(Kind::Block),
        "/symskills:only" => Some(Kind::Only),
        "/symskills:except" => Some(Kind::Except),
        _ => None,
    }
}
fn marker_probe(line: &str) -> bool {
    let bytes = line.as_bytes();
    for start in 0..bytes.len() {
        if &bytes[start..].get(..4).unwrap_or_default() != b"<!--" {
            continue;
        }
        let mut index = start + 4;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) == Some(&b'/') {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                index += 1;
            }
        }
        if bytes
            .get(index..)
            .is_some_and(|rest| rest.starts_with(b"symskills:"))
        {
            return true;
        }
    }
    false
}
#[allow(
    clippy::too_many_lines,
    reason = "the flat state machine keeps marker error ordering explicit"
)]
fn parse(src: &str) -> (Vec<Segment>, Vec<Problem>) {
    let mut segments = Vec::new();
    let mut literal = Vec::new();
    let mut open: Option<Segment> = None;
    let mut seen = BTreeMap::new();
    let mut problems = Vec::new();
    let flush = |segments: &mut Vec<Segment>, literal: &mut Vec<String>| {
        if !literal.is_empty() {
            segments.push(Segment {
                region: false,
                kind: Kind::Block,
                id: String::new(),
                targets: Vec::new(),
                line: 0,
                lines: std::mem::take(literal),
            });
        }
    };
    for (index, line) in src.split('\n').enumerate() {
        let line_no = index + 1;
        if let Some((kind, id, targets)) = opening(line) {
            if kind == Kind::Block && !block_id(&id) {
                problems.push(problem(CODE_BLOCK_ID_INVALID, format!("block id {id:?} must be lowercase alphanumeric segments joined by single dashes"), line_no));
                continue;
            }
            if let Some(current) = open.as_ref() {
                let opening_label = if kind == Kind::Block {
                    format!("block {id:?}")
                } else {
                    format!("{} region", kind_name(kind))
                };
                problems.push(problem(
                    CODE_BLOCK_NESTED,
                    format!(
                        "{opening_label} opens inside the {} region opened on line {}; regions must not nest",
                        kind_name(current.kind),
                        current.line
                    ),
                    line_no,
                ));
                continue;
            }
            if kind == Kind::Block {
                if let Some(previous) = seen.get(&id) {
                    problems.push(problem(
                        CODE_BLOCK_DUPLICATE_ID,
                        format!(
                            "block id {id:?} is already used on line {previous}; ids must be unique per file"
                        ),
                        line_no,
                    ));
                    continue;
                }
                seen.insert(id.clone(), line_no);
            }
            if !matches!(kind, Kind::Block) && targets.is_empty() {
                problems.push(problem(
                    CODE_TARGET_LIST_EMPTY,
                    format!("{} region lists no target names", kind_name(kind)),
                    line_no,
                ));
                continue;
            }
            flush(&mut segments, &mut literal);
            open = Some(Segment {
                region: true,
                kind,
                id,
                targets,
                line: line_no,
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(kind) = closing(line) {
            if let Some(current) = open.take() {
                if current.kind == kind {
                    segments.push(current);
                } else {
                    problems.push(problem(
                        CODE_BLOCK_CLOSE_MISMATCH,
                        format!(
                            "closing {} marker ends the {} region opened on line {}",
                            kind_name(kind),
                            kind_name(current.kind),
                            current.line
                        ),
                        line_no,
                    ));
                    open = Some(current);
                }
            } else {
                problems.push(problem(
                    CODE_BLOCK_UNMATCHED_CLOSE,
                    format!(
                        "closing {} marker without a matching opening marker",
                        kind_name(kind)
                    ),
                    line_no,
                ));
            }
            continue;
        }
        if marker_probe(line) {
            problems.push(problem(
                CODE_MARKER_MALFORMED,
                format!("unrecognised symskills marker: {}", line.trim()),
                line_no,
            ));
            continue;
        }
        if let Some(current) = open.as_mut() {
            current.lines.push(line.into());
        } else {
            literal.push(line.into());
        }
    }
    if let Some(mut current) = open {
        problems.push(problem(
            CODE_BLOCK_UNCLOSED,
            format!(
                "{} region opened on line {} is never closed",
                if current.id.is_empty() {
                    kind_name(current.kind).to_string()
                } else {
                    format!("block {:?}", current.id)
                },
                current.line
            ),
            current.line,
        ));
        current.region = false;
        segments.push(current);
    }
    flush(&mut segments, &mut literal);
    (segments, problems)
}

fn terms_in(src: &str) -> (Vec<String>, Vec<Problem>) {
    let mut names = BTreeSet::new();
    let mut problems = Vec::new();
    let mut rest = src;
    while let Some(start) = rest.find("{{term:") {
        let after = &rest[start + 7..];
        let Some(end) = after.find("}}") else { break };
        let raw = &after[..end];
        let name = raw.trim();
        if term_name(name) {
            names.insert(name.to_string());
        } else {
            problems.push(problem(
                CODE_TERM_NAME_INVALID,
                format!("term reference \"{{{{term:{raw}}}}}\" must be lowercase alphanumeric segments joined by single dashes or underscores"),
                0,
            ));
        }
        rest = &after[end + 2..];
    }
    (names.into_iter().collect(), problems)
}

#[must_use]
/// Public operation `scan_text`.
pub fn scan_text(src: &str) -> (Scan, Vec<Problem>) {
    let (segments, mut problems) = parse(src);
    let mut scan = Scan::default();
    for segment in segments {
        if segment.region {
            scan.regions.push(Region {
                kind: segment.kind,
                id: segment.id.clone(),
                targets: segment.targets,
                line: segment.line,
            });
            if segment.kind == Kind::Block {
                scan.block_ids.push(segment.id);
            }
        }
    }
    let (terms, term_problems) = terms_in(src);
    scan.terms = terms;
    problems.extend(term_problems);
    scan.block_ids.sort();
    (scan, problems)
}

#[path = "variant_ops.rs"]
mod variant_ops;
pub use variant_ops::{
    apply, check_overrides, check_region_targets, check_terms, find_mentions, unscoped_text,
};
