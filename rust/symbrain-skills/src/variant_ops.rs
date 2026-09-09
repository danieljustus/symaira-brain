use std::collections::{BTreeMap, BTreeSet};

use super::{
    BLOCKS_DIR, CODE_OVERRIDE_UNKNOWN, CODE_TARGET_UNKNOWN, CODE_TERM_DEFAULT_REQUIRED,
    CODE_TERM_NAME_INVALID, CODE_TERM_UNKNOWN, DEFAULT_KEY, Kind, Mention, Options, Problem,
    Region, Result, SEVERITY_ERROR, SEVERITY_WARNING, closing, opening, parse, problem, term_name,
};

/// Public operation `apply`.
pub fn apply(src: &str, options: &Options) -> (Result, Vec<Problem>) {
    let (segments, mut problems) = parse(src);
    let mut result = Result {
        source_bytes: src.len(),
        ..Result::default()
    };
    let mut output = Vec::new();
    for segment in segments {
        if !segment.region {
            output.extend(segment.lines);
            continue;
        }
        match segment.kind {
            Kind::Block => {
                if let Some(override_text) = options.overrides.get(&segment.id) {
                    result.blocks.push(segment.id);
                    let text = override_text.trim_end_matches('\n');
                    if !text.trim().is_empty() {
                        result.replaced_bytes += text.len();
                        output.extend(text.split('\n').map(ToString::to_string));
                    }
                } else {
                    output.extend(segment.lines);
                }
            }
            Kind::Only => {
                if segment
                    .targets
                    .iter()
                    .any(|target| target == &options.target)
                {
                    output.extend(segment.lines);
                }
            }
            Kind::Except => {
                if !segment
                    .targets
                    .iter()
                    .any(|target| target == &options.target)
                {
                    output.extend(segment.lines);
                }
            }
        }
    }
    result.blocks.sort();
    let mut text = output.join("\n");
    let (resolved, used, term_problems) = substitute_terms(&text, options);
    text = resolved;
    result.terms = used;
    result.replaced_bytes += result.terms.values().map(String::len).sum::<usize>();
    result.text = text;
    problems.extend(term_problems);
    (result, problems)
}
fn substitute_terms(
    src: &str,
    options: &Options,
) -> (String, BTreeMap<String, String>, Vec<Problem>) {
    let mut output = String::new();
    let mut used = BTreeMap::new();
    let mut problems = Vec::new();
    let mut reported = BTreeSet::new();
    let mut rest = src;
    while let Some(start) = rest.find("{{term:") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 7..];
        let Some(end) = after.find("}}") else {
            output.push_str(&rest[start..]);
            break;
        };
        let raw = &after[..end];
        let original = &rest[start..start + 7 + end + 2];
        let name = raw.trim();
        let replacement = if !term_name(name) {
            if reported.insert(original.to_string()) {
                problems.push(problem(
                    CODE_TERM_NAME_INVALID,
                    format!("term reference {original:?} must be lowercase alphanumeric segments joined by single dashes or underscores"),
                    0,
                ));
            }
            original.to_string()
        } else if let Some(values) = options.terms.get(name) {
            if let Some(value) = values
                .get(&options.target)
                .filter(|value| !value.is_empty())
            {
                used.insert(name.into(), value.clone());
                value.clone()
            } else if let Some(value) = values.get(DEFAULT_KEY).filter(|value| !value.is_empty()) {
                used.insert(name.into(), value.clone());
                value.clone()
            } else {
                if reported.insert(name.to_string()) {
                    problems.push(problem(
                        CODE_TERM_DEFAULT_REQUIRED,
                        format!("term {name:?} needs a {DEFAULT_KEY:?} value; it is the harness-neutral text the canonical source states"),
                        0,
                    ));
                }
                original.to_string()
            }
        } else {
            if reported.insert(name.to_string()) {
                problems.push(problem(
                    CODE_TERM_UNKNOWN,
                    format!("term {name:?} is referenced but not defined in [terms]"),
                    0,
                ));
            }
            original.to_string()
        };
        output.push_str(&replacement);
        rest = &after[end + 2..];
    }
    if !rest.is_empty() && !rest.contains("{{term:") {
        output.push_str(rest);
    }
    (output, used, problems)
}

#[must_use]
/// Public operation `unscoped_text`.
pub fn unscoped_text(src: &str) -> String {
    let mut output = vec![String::new(); src.split('\n').count()];
    let mut only = false;
    let mut fence = false;
    for (index, line) in src.split('\n').enumerate() {
        let trimmed = line.trim();
        if let Some((kind, _, _)) = opening(line) {
            if kind == Kind::Only {
                only = true;
            }
            continue;
        }
        if let Some(kind) = closing(line) {
            if kind == Kind::Only {
                only = false;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !only && !fence {
            output[index] = line.into();
        }
    }
    output.join("\n")
}
fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
fn contains_word(line: &str, name: &str) -> bool {
    let line = line.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(index) = line[offset..].find(&name) {
        let start = offset + index;
        let end = start + name.len();
        let left = line[..start].chars().next_back().is_none_or(|c| !word(c));
        let right = line[end..].chars().next().is_none_or(|c| !word(c));
        if left && right {
            return true;
        }
        offset = end;
    }
    false
}
#[must_use]
/// Public operation `find_mentions`.
pub fn find_mentions(src: &str, names: &[String]) -> Vec<Mention> {
    let lines = unscoped_text(src);
    let mut mentions = Vec::new();
    for name in names {
        for (line, text) in lines.split('\n').enumerate() {
            if !text.is_empty() && contains_word(text, name) {
                mentions.push(Mention {
                    name: name.clone(),
                    line: line + 1,
                });
                break;
            }
        }
    }
    mentions.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.name.cmp(&right.name))
    });
    mentions
}
#[must_use]
/// Public operation `check_overrides`.
pub fn check_overrides(
    source_ids: &[String],
    overrides: &BTreeMap<String, Vec<String>>,
) -> Vec<Problem> {
    let known = source_ids.iter().collect::<BTreeSet<_>>();
    let mut problems = Vec::new();
    for (target, ids) in overrides {
        for id in ids {
            if !known.contains(id) {
                problems.push(Problem { code: CODE_OVERRIDE_UNKNOWN.into(), severity: SEVERITY_ERROR.into(), message: format!("overlay {target}/{BLOCKS_DIR}/{id}.md overrides block {id:?}, which no SKILL.md or markdown reference in this skill defines"), line: 0 });
            }
        }
    }
    problems
}
#[must_use]
/// Public operation `check_region_targets`.
pub fn check_region_targets(regions: &[Region], known: &[String]) -> Vec<Problem> {
    if known.is_empty() {
        return Vec::new();
    }
    regions
        .iter()
        .flat_map(|region| {
            region
                .targets
                .iter()
                .filter(|target| !known.contains(target))
                .map(|target| Problem {
                    code: CODE_TARGET_UNKNOWN.into(),
                    severity: SEVERITY_WARNING.into(),
                    message: format!(
                        "{} region names unknown target {target:?}; the region is kept or dropped as if that target never renders",
                        match region.kind {
                            Kind::Block => "block",
                            Kind::Only => "only",
                            Kind::Except => "except",
                        }
                    ),
                    line: region.line,
                })
        })
        .collect()
}
#[must_use]
/// Public operation `check_terms`.
pub fn check_terms(
    terms: &BTreeMap<String, BTreeMap<String, String>>,
    known: &[String],
) -> Vec<Problem> {
    let known = known.iter().collect::<BTreeSet<_>>();
    let mut problems = Vec::new();
    for (name, values) in terms {
        if !term_name(name) {
            problems.push(Problem {
                code: CODE_TERM_NAME_INVALID.into(),
                severity: SEVERITY_ERROR.into(),
                message: format!("term name {name:?} must be lowercase alphanumeric segments joined by single dashes or underscores"),
                line: 0,
            });
        }
        if values
            .get(DEFAULT_KEY)
            .is_none_or(|value| value.trim().is_empty())
        {
            problems.push(Problem {
                code: CODE_TERM_DEFAULT_REQUIRED.into(),
                severity: SEVERITY_ERROR.into(),
                message: format!("term {name:?} needs a {DEFAULT_KEY:?} value; it is the harness-neutral text the canonical source states"),
                line: 0,
            });
        }
        for key in values.keys() {
            if key != DEFAULT_KEY && !known.contains(key) {
                problems.push(Problem {
                    code: CODE_TARGET_UNKNOWN.into(),
                    severity: SEVERITY_WARNING.into(),
                    message: format!("term {name:?} defines a value for unknown target {key:?}; it will never be used"),
                    line: 0,
                });
            }
        }
    }
    problems
}
