//! Text shaping helpers of the assembler. Go sliced strings by byte; here the
//! cut moves down to a character boundary instead of splitting a code point.

use super::Recalled;
use super::budget::word_token_estimate;

/// Degradation level of one retrieval item (#414).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Full,
    Summary,
    Reference,
}

/// `s[..max]` at a character boundary.
pub(crate) fn prefix(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `truncateText`: at most `max` bytes, then an ellipsis.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    format!("{}…", prefix(text, max))
}

/// Extractive summary: cut at a sentence or word boundary.
pub(crate) fn summarize_content(content: &str, max: usize) -> String {
    if content.len() <= max {
        return content.to_owned();
    }
    let trimmed = prefix(content, max);
    if let Some(index) = trimmed.rfind(". ")
        && index > max / 2
    {
        return content[..=index].to_owned();
    }
    if let Some(index) = trimmed.rfind(' ')
        && index > 0
    {
        return format!("{}…", &content[..index]);
    }
    format!("{trimmed}…")
}

/// First line or sentence.
pub(crate) fn first_line(content: &str) -> String {
    let content = content.trim();
    if let Some(index) = content.find('\n')
        && index > 0
    {
        return content[..index].to_owned();
    }
    if let Some(index) = content.find(". ")
        && index > 0
    {
        return content[..=index].to_owned();
    }
    if content.len() > 60 {
        return format!("{}…", prefix(content, 60));
    }
    content.to_owned()
}

pub(crate) fn single_result(item: &Recalled, level: Level) -> String {
    match level {
        Level::Full => format!("- {}", item.content),
        Level::Summary => format!(
            "- {} [ref: {}]",
            summarize_content(&item.content, 80),
            item.id
        ),
        Level::Reference => format!("- [{}] {}", item.id, first_line(&item.content)),
    }
}

/// Legacy single block at full level (at most ten items).
pub(crate) fn retrieval_block(items: &[Recalled]) -> String {
    let mut out = String::from("Relevant stored memories:\n");
    for item in items.iter().take(10) {
        out.push_str(&format!("- {}\n", item.content));
    }
    out
}

pub(crate) fn working_block(items: &[Recalled]) -> String {
    let mut out = String::from("Active working memories:\n");
    for item in items.iter().take(20) {
        out.push_str(&format!("- {}\n", item.content));
    }
    out
}

/// Compact reference-only block for when the full block cannot fit.
pub(crate) fn degraded_block(items: &[Recalled], budget: i64) -> (String, i64) {
    let lines: Vec<String> = items
        .iter()
        .map(|r| format!("- [{}] {}", r.id, truncate(&r.content, 100)))
        .collect();
    let mut content = format!("Relevant memories (reference):\n{}", lines.join("\n"));
    let mut tokens = word_token_estimate(&content);
    if tokens > budget {
        let mut few: Vec<String> = Vec::new();
        for item in items {
            let mut next = few.clone();
            next.push(format!("- {}", truncate(&item.content, 40)));
            if word_token_estimate(&format!("Relevant memories:\n{}", next.join("\n"))) > budget {
                break;
            }
            few = next;
        }
        content = format!("Relevant memories:\n{}", few.join("\n"));
        tokens = word_token_estimate(&content);
    }
    (content, tokens)
}

/// Last `max_turns * 2` lines of the session text.
pub(crate) fn working_context(session_text: &str, max_turns: i64) -> String {
    let lines: Vec<&str> = session_text.split('\n').collect();
    let keep = usize::try_from(max_turns).unwrap_or(0).saturating_mul(2);
    if lines.len() <= keep {
        return session_text.to_owned();
    }
    lines[lines.len() - keep..].join("\n")
}
