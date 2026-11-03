//! go-yaml (`gopkg.in/yaml.v3`) diagnostics and scalar text for SKILL.md
//! frontmatter, rebuilt from the parser the native loader uses.
//!
//! The native loader decodes with `serde_yaml_ng`; Go prints different words
//! for the same mistakes (`yaml: line N: ...` instead of `... at line N column
//! M`). These helpers translate the common cases so a damaged frontmatter is
//! reported with Go's bytes.

use std::collections::BTreeMap;

/// Go's `parser.fail` text for a libyaml syntax error reported by serde.
///
/// serde prints `PROBLEM at line L column C[, while CONTEXT at line CL column
/// CC]` with 1-based lines; libyaml marks (and go-yaml's arithmetic) are
/// 0-based, and a scanner error does not advance the line before reporting.
pub(crate) fn syntax_error(serde_message: &str) -> Option<String> {
    if let Some((problem, _)) = serde_message.split_once(" at position ") {
        // Reader errors carry no line.
        return Some(format!("yaml: {problem}"));
    }
    let (head, context) = match serde_message.split_once(", while ") {
        Some((head, context)) => (head, Some(context)),
        None => (serde_message, None),
    };
    let (problem, position) = head.rsplit_once(" at line ")?;
    let problem_line = position.split_once(" column ")?.0.parse::<usize>().ok()? - 1;
    let context_line = context
        .and_then(|context| context.rsplit_once(" at line "))
        .and_then(|(_, rest)| rest.split_once(" column "))
        .and_then(|(line, _)| line.parse::<usize>().ok())
        .map_or(0, |line| line - 1);
    let scanner = context.is_none_or(|context| context.starts_with("scanning"));
    let mut line = if context_line != 0 {
        context_line
    } else {
        problem_line
    };
    if line != 0 && scanner {
        line += 1;
    }
    let where_ = if line == 0 {
        String::new()
    } else {
        format!("line {line}: ")
    };
    Some(format!("yaml: {where_}{problem}"))
}

/// go-yaml's `yaml: unmarshal errors:` text for repeated top-level plain keys.
pub(crate) fn duplicate_key_text(text: &str) -> Option<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut lines = String::new();
    for (index, line) in text.lines().enumerate() {
        if line.starts_with([' ', '\t', '#', '-']) {
            continue;
        }
        let Some((key, _)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim_end();
        if key.is_empty() || key.starts_with(['"', '\'', '[', '{', '?', '&', '*', '!', '|', '>']) {
            continue;
        }
        if let Some(first) = seen.get(key) {
            lines.push_str(&format!(
                "\n  line {}: mapping key \"{key}\" already defined at line {first}",
                index + 1
            ));
        } else {
            seen.insert(key, index + 1);
        }
    }
    (!lines.is_empty()).then(|| format!("yaml: unmarshal errors:{lines}"))
}

/// The document root is not a mapping: go-yaml's `terror` for
/// `map[string]yaml.Node`. `tag` is the short YAML tag (`!!str`, `!!seq`, ...).
pub(crate) fn not_a_mapping(tag: &str, value: &str) -> String {
    let shown = if tag == "!!seq" || tag == "!!map" {
        String::new()
    } else if value.len() > 10 {
        let head: String = value.chars().take(7).collect();
        format!(" `{head}...`")
    } else {
        format!(" `{value}`")
    };
    format!(
        "yaml: unmarshal errors:\n  line 1: cannot unmarshal {tag}{shown} into map[string]yaml.Node"
    )
}

/// The source text of a single-line top-level plain scalar (`key: 1.50`),
/// which go-yaml keeps verbatim when decoding into a string.
pub(crate) fn raw_plain_scalar(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let line = text.lines().find(|line| line.starts_with(&prefix))?;
    let value = line[prefix.len()..].trim();
    let value = value
        .split_once(" #")
        .map_or(value, |(head, _)| head)
        .trim();
    if value.is_empty() || value.starts_with(['"', '\'', '&', '*', '!', '|', '>', '[', '{']) {
        return None;
    }
    Some(value.to_owned())
}
