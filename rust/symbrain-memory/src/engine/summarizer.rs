//! Offline extractive summaries with original Go byte-valued output behavior.

use std::collections::BTreeSet;

use super::text::{decode_runes, lower, split_sentences, trim};

struct Sentence {
    text: String,
    score: f64,
    index: usize,
}

/// Chooses the most informational sentences and restores chronological order.
///
/// Output remains bytes: Go uppercases the first byte as a rune and appends the
/// original remaining bytes, which can yield malformed UTF-8 for a non-ASCII
/// initial character. Converting that output to a lossy String changes its
/// contract. The nonnegative limit is the domain used by both Go consumers.
#[must_use]
pub fn summarize_session(session: &[u8], max_sentences: usize) -> Vec<u8> {
    let session = decode_runes(session);
    if trim(&session).is_empty() {
        return Vec::new();
    }
    let mut sentences = Vec::new();
    for line in session.split('\n') {
        let line = strip_role(trim(line));
        for part in split_sentences(line, false) {
            let part = trim(part);
            if part.len() > 12 {
                sentences.push(part.to_string());
            }
        }
    }
    if sentences.is_empty() {
        return Vec::new();
    }
    let total = sentences.len();
    let mut scores: Vec<_> = sentences
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let score = score_sentence(&text, index, total);
            Sentence { text, score, index }
        })
        .collect();
    // Preserve Go's pairwise swaps: a stable sort changes some tied rankings.
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
    let mut output = b"Session Context Summary:\n".to_vec();
    let mut seen = BTreeSet::new();
    for sentence in scores {
        let text = sentence.text.trim_matches(['*', '_', '-', ' ']);
        if let Some((&first, rest)) = text.as_bytes().split_first() {
            let character = char::from(first);
            let mut upper = character.to_uppercase();
            let initial = upper.next().unwrap_or(character);
            // Go simple uppercase does not expand Latin-1 sharp s to SS.
            let initial = if upper.next().is_some() {
                character
            } else {
                initial
            };
            let mut cleaned = initial.to_string().into_bytes();
            cleaned.extend_from_slice(rest);
            if seen.insert(cleaned.clone()) {
                output.extend_from_slice(b"- ");
                output.extend(cleaned);
                output.push(b'\n');
            }
        }
    }
    output
}

fn strip_role(text: &str) -> &str {
    let folded = lower(text);
    for prefix in [
        "user:",
        "assistant:",
        "agent:",
        "system:",
        "human:",
        "ai:",
        "bot:",
    ] {
        if folded.starts_with(prefix) {
            return trim(&text[prefix.len()..]);
        }
    }
    text
}

fn score_sentence(text: &str, index: usize, total: usize) -> f64 {
    let lower = lower(text);
    let mut score = if text.len() > 25 && text.len() < 120 {
        3.0
    } else if text.len() >= 120 {
        1.5
    } else {
        0.0
    };
    for keyword in [
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
    ] {
        if lower.contains(keyword) {
            score += 2.0;
        }
    }
    for indicator in [
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
    ] {
        if lower.contains(indicator) {
            score += 4.0;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let position = index as f64 / total as f64;
    if position < 0.15 {
        score += 2.0;
    } else if position > 0.80 {
        score += 3.0;
    }
    for padding in [
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
    ] {
        if lower.contains(padding) {
            score -= 2.5;
        }
    }
    score
}
