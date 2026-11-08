//! Markdown helpers shared by the Codex memory importers: bounded file reads,
//! flat frontmatter parsing, application lists and `## Section` extraction.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_MARKDOWN_BYTES: usize = 1 << 20;

pub(super) fn read_markdown(path: &Path) -> std::io::Result<String> {
    let mut data = std::fs::read(path)?;
    data.truncate(MAX_MARKDOWN_BYTES);
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// `parseMarkdown`: flat frontmatter map and the remaining body.
pub(super) fn parse_markdown(content: &str) -> (BTreeMap<String, String>, String) {
    let mut front = BTreeMap::new();
    let content = content.replace("\r\n", "\n");
    let Some(rest) = content.strip_prefix("---\n") else {
        return (front, content);
    };
    let Some(end) = rest.find("\n---") else {
        return (front, content);
    };
    let mut current = String::new();
    for line in rest[..end].split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed
            .strip_prefix('-')
            .filter(|_| current == "applications")
        {
            let item = item.trim();
            if !item.is_empty() {
                let slot: &mut String = front.entry(current.clone()).or_default();
                if !slot.is_empty() {
                    slot.push(',');
                }
                slot.push_str(item);
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            current.clear();
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            current.clear();
            continue;
        }
        key.clone_into(&mut current);
        front.insert(
            key.to_owned(),
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim()
                .to_owned(),
        );
    }
    let body = &rest[end + 4..];
    (front, body.strip_prefix('\n').unwrap_or(body).to_owned())
}

pub(super) fn clean_applications(values: impl Iterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = values
        .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .filter(|v| !v.is_empty())
        .collect();
    set.into_iter().collect()
}

pub(super) fn parse_applications(raw: &str) -> Vec<String> {
    let mut raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    if raw.starts_with('[') && raw.ends_with(']') {
        if let Ok(values) = serde_json::from_str::<Vec<String>>(raw) {
            return clean_applications(values.into_iter());
        }
        raw = raw[1..raw.len() - 1].trim();
    }
    clean_applications(raw.split(',').map(str::to_owned))
}

pub(super) fn section_body(body: &str, heading: &str) -> String {
    let lines: Vec<&str> = body.split('\n').collect();
    let want = format!("## {heading}");
    let Some(start) = lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(&want))
        .map(|i| i + 1)
    else {
        return String::new();
    };
    let end = lines[start..]
        .iter()
        .position(|l| l.trim().starts_with("## "))
        .map_or(lines.len(), |i| start + i);
    lines[start..end].join("\n").trim().to_owned()
}
