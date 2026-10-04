//! Deterministic fuzzy alignment from pinned evidencekit's weighted LCS.

use super::{Span, go_space, span};

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

/// Scores windows of evidence-token count minus one through plus one.
/// Ties retain the first window: shorter widths, then earlier byte positions.
#[must_use]
pub fn align_fuzzy(source: &str, evidence: &str) -> Option<Span> {
    let evidence = tokenize(evidence);
    let source = tokenize(source);
    if evidence.is_empty() || source.is_empty() {
        return None;
    }
    let mut best = 0.0;
    let mut best_span = None;
    for width in evidence.len().saturating_sub(1)..=evidence.len().saturating_add(1) {
        if width == 0 || width > source.len() {
            continue;
        }
        for window in source.windows(width) {
            let score = sequence_score(&evidence, window);
            if score > best {
                best = score;
                best_span = span(window[0].start, window[width - 1].end);
            }
        }
    }
    if best < 0.6 { None } else { best_span }
}

fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (offset, ch) in text.char_indices() {
        if go_space(ch) {
            if let Some(begin) = start.take() {
                tokens.push(Token {
                    text: &text[begin..offset],
                    start: begin,
                    end: offset,
                });
            }
        } else if start.is_none() {
            start = Some(offset);
        }
    }
    if let Some(begin) = start {
        tokens.push(Token {
            text: &text[begin..],
            start: begin,
            end: text.len(),
        });
    }
    tokens
}

#[allow(clippy::cast_precision_loss)] // Token counts are the Go float64 denominator.
fn sequence_score(left: &[Token<'_>], right: &[Token<'_>]) -> f64 {
    let mut previous = vec![0.0_f64; right.len() + 1];
    let mut current = previous.clone();
    for left_token in left {
        for (index, right_token) in right.iter().enumerate() {
            let j = index + 1;
            let mut best = previous[j].max(current[j - 1]);
            let similarity = token_similarity(left_token.text, right_token.text);
            if similarity >= 0.6 {
                best = best.max(previous[j - 1] + similarity);
            }
            current[j] = best;
        }
        std::mem::swap(&mut current, &mut previous);
    }
    2.0 * previous[right.len()] / (left.len() + right.len()) as f64
}

#[allow(clippy::cast_precision_loss)] // Same rune edit-distance ratio as Go.
fn token_similarity(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let maximum = left.len().max(right.len());
    if maximum == 0 {
        return 1.0;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (i, left_ch) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_ch) in right.iter().enumerate() {
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(left_ch != right_ch));
        }
        std::mem::swap(&mut current, &mut previous);
    }
    1.0 - previous[right.len()] as f64 / maximum as f64
}
