//! Context budgeting with the original positional pinned-piece semantics.

use super::text::decode_runes;

/// An assembled layer and its caller-supplied token estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetPiece {
    /// Original Go layer bytes, including custom layer names.
    pub layer: Vec<u8>,
    pub tokens: i64,
}

/// Observation of a hard budget. `fit` records whether the original set fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetReport {
    pub max_tokens: i64,
    pub estimated_tokens: i64,
    pub dropped_pieces: usize,
    pub dropped_ids: Vec<Vec<u8>>,
    pub reason: &'static str,
    pub fit: bool,
}

/// Estimates Go rune count / 4 + 1, counting each newline once more.
#[must_use]
pub fn estimate_tokens(text: &[u8]) -> usize {
    if text.is_empty() {
        return 0;
    }
    decode_runes(text).chars().count() / 4 + 1 + text.iter().filter(|byte| **byte == b'\n').count()
}

/// Drops from the end, always retaining the first two pieces regardless of name.
/// Returns no report and leaves input intact for a nonpositive limit.
pub fn enforce_budget(pieces: &mut Vec<BudgetPiece>, max_tokens: i64) -> Option<BudgetReport> {
    if max_tokens <= 0 {
        return None;
    }
    let mut total = pieces
        .iter()
        .fold(0_i64, |sum, piece| sum.wrapping_add(piece.tokens));
    let fit = total <= max_tokens;
    let mut report = BudgetReport {
        max_tokens,
        estimated_tokens: total,
        dropped_pieces: 0,
        dropped_ids: Vec::new(),
        reason: "all pieces fit within the budget",
        fit,
    };
    if fit {
        return Some(report);
    }
    while pieces.len() > 2 && total > max_tokens {
        if let Some(piece) = pieces.pop() {
            total = total.wrapping_sub(piece.tokens);
            report.dropped_ids.push(piece.layer);
            report.dropped_pieces += 1;
        }
    }
    report.estimated_tokens = total;
    report.reason = "hard budget: dropped lowest-priority pieces first; working context and working memory are never dropped";
    Some(report)
}
