//! Offline extractive session summarizer (ported for #760 from
//! `internal/memory/summarizer/summary.go` at v0.12.0).
//!
//! Splits a dialog into sentences, scores them with fixed keyword/position
//! heuristics, keeps the best `max_sentences` in chronological order and
//! renders a bullet list. No network, no clock.
//!
//! Deviation from Go: the first letter is upper-cased as a whole character;
//! Go upper-cased only the first *byte*, which corrupted multibyte starts.

const ROLE_PREFIXES: [&str; 7] = [
    "user:",
    "assistant:",
    "agent:",
    "system:",
    "human:",
    "ai:",
    "bot:",
];

const KEYWORDS: [&str; 28] = [
    "symaira",
    "vault",
    "memory",
    "project",
    "projekt",
    "preference",
    "bevorzuge",
    "like",
    "work",
    "build",
    "baue",
    "agent",
    "mcp",
    "typescript",
    "python",
    "golang",
    "sqlite",
    "todo",
    "done",
    "erledigt",
    "nächstes",
    "ziel",
    "goal",
    "roadmap",
    "sync",
    "cloud",
    "private",
    "public",
];

const ACTIONS: [&str; 12] = [
    "i will",
    "ich werde",
    "let's",
    "wir sollten",
    "plan is",
    "plan ist",
    "resolved",
    "finished",
    "erledigt",
    "erstellt",
    "created",
    "setup",
];

const PADDINGS: [&str; 18] = [
    "hello",
    "hallo",
    "hi ",
    "thank you",
    "danke",
    "bitte",
    "please",
    "super",
    "great",
    "awesome",
    "coole",
    "toll",
    "yes",
    "no",
    "ja",
    "nein",
    "ok",
    "okay",
];

struct Scored {
    text: String,
    score: f64,
    index: usize,
}

fn strip_role_labels(text: &str) -> &str {
    let lower = text.to_lowercase();
    for prefix in ROLE_PREFIXES {
        if lower.starts_with(prefix) {
            return text.get(prefix.len()..).unwrap_or(text).trim();
        }
    }
    text
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if matches!(c, '.' | '?' | '!') {
            if !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn score_sentence(text: &str, index: usize, total: usize) -> f64 {
    let lower = text.to_lowercase();
    let mut score = 0.0;
    let length = text.len();
    if length > 25 && length < 120 {
        score += 3.0;
    } else if length >= 120 {
        score += 1.5;
    }
    let hits = |list: &[&str]| list.iter().filter(|w| lower.contains(**w)).count() as f64;
    score += 2.0 * hits(&KEYWORDS);
    score += 4.0 * hits(&ACTIONS);
    let position = index as f64 / total as f64;
    if position < 0.15 {
        score += 2.0;
    } else if position > 0.80 {
        score += 3.0;
    }
    score - 2.5 * hits(&PADDINGS)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Summarizes `session_text` into at most `max_sentences` bullets; empty
/// input (or no sentence longer than 12 bytes) gives an empty string.
#[must_use]
pub fn summarize_session(session_text: &str, max_sentences: usize) -> String {
    if session_text.trim().is_empty() {
        return String::new();
    }
    let mut raw: Vec<String> = Vec::new();
    for line in session_text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        for part in split_sentences(strip_role_labels(line)) {
            let part = part.trim();
            if part.len() > 12 {
                raw.push(part.to_owned());
            }
        }
    }
    if raw.is_empty() {
        return String::new();
    }
    let total = raw.len();
    let mut scores: Vec<Scored> = raw
        .into_iter()
        .enumerate()
        .map(|(index, text)| Scored {
            score: score_sentence(&text, index, total),
            text,
            index,
        })
        .collect();
    // The exchange sorts are kept literally: they are not stable, and the
    // tie order is part of the observable output.
    for i in 0..scores.len() {
        for j in i + 1..scores.len() {
            if scores[j].score > scores[i].score {
                scores.swap(i, j);
            }
        }
    }
    scores.truncate(max_sentences);
    for i in 0..scores.len() {
        for j in i + 1..scores.len() {
            if scores[j].index < scores[i].index {
                scores.swap(i, j);
            }
        }
    }
    let mut out = String::from("Session Context Summary:\n");
    let mut seen: Vec<String> = Vec::new();
    for sentence in &scores {
        let trimmed = sentence
            .text
            .trim_matches(|c| matches!(c, '*' | '_' | '-' | ' '));
        if trimmed.is_empty() {
            continue;
        }
        let cleaned = capitalize(trimmed);
        if !seen.contains(&cleaned) {
            out.push_str("- ");
            out.push_str(&cleaned);
            out.push('\n');
            seen.push(cleaned);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIALOGUE: &str = "\nUser: Hello! I want to start a new project.\nAssistant: Hi! What kind of project is it?\nUser: I am building symaira-memory today.\nAssistant: That sounds interesting! What tools will you use?\nUser: I will use Go, SQLite, and a custom TUI built with Bubble Tea.\nAssistant: Excelent choice!\nUser: The goal is to finish the core Phase 1.\n";

    #[test]
    fn go_session_dialogue() {
        let summary = summarize_session(DIALOGUE, 3).to_lowercase();
        assert!(summary.contains("session context summary:"));
        assert!(!summary.contains("hello"));
        assert!(summary.contains("symaira-memory"));
        assert!(summary.contains("bubble tea"));
    }

    #[test]
    fn exact_output_is_deterministic_and_chronological() {
        let got = summarize_session(DIALOGUE, 2);
        assert_eq!(got, summarize_session(DIALOGUE, 2));
        let lines: Vec<_> = got.lines().collect();
        assert_eq!(lines.len(), 3, "{got}");
        assert!(lines[1].starts_with("- ") && lines[2].starts_with("- "));
        let first = DIALOGUE
            .find(&lines[1][2..])
            .expect("first bullet in dialog");
        let second = DIALOGUE
            .find(&lines[2][2..])
            .expect("second bullet in dialog");
        assert!(first < second);
    }

    #[test]
    fn edge_cases() {
        for (input, max) in [("", 3), ("   \n ", 3), ("User: short.", 3), ("a. b. c.", 3)] {
            assert_eq!(summarize_session(input, max), "", "{input:?}");
        }
        assert_eq!(
            summarize_session("User: ich baue ein projekt für symaira.", 0),
            "Session Context Summary:\n"
        );
        let dup = "System: das projekt ist erledigt und erstellt.\nHuman: das projekt ist erledigt und erstellt.";
        assert_eq!(
            summarize_session(dup, 5),
            "Session Context Summary:\n- Das projekt ist erledigt und erstellt\n"
        );
        assert!(summarize_session("**élan vital und projekt**", 1).contains("- Élan vital"));
    }
}
