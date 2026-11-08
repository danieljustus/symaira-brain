//! Token estimation and the hard budget pass (`budget.go`, #492).

use super::AssembledContext;
use serde::Serialize;

/// Runes/4 + 1, plus one per newline — the long-standing heuristic. It
/// over-estimates code-heavy text, the safe direction for a budget guard.
#[must_use]
pub fn default_token_estimate(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    let runes = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
    let newlines = i64::try_from(text.matches('\n').count()).unwrap_or(0);
    runes / 4 + 1 + newlines
}

/// The legacy word heuristic `len(fields)*4/3`.
pub(crate) fn word_token_estimate(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    i64::try_from(text.split_whitespace().count() * 4 / 3).unwrap_or(0)
}

/// What a hard token budget did to an assembly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BudgetReport {
    /// Enforced ceiling.
    pub max_tokens: i64,
    /// Final estimated total.
    pub estimated_tokens: i64,
    /// Pieces dropped.
    pub dropped_pieces: i64,
    /// Layers of the dropped pieces, in drop order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped_ids: Vec<String>,
    /// Why.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// Everything fit.
    pub fit: bool,
}

const DROP_ORDER_REASON: &str = "hard budget: dropped lowest-priority pieces first; working context and working memory are never dropped";

/// Trims `ctx.pieces` to `max_tokens` and fills `ctx.budget_report`. The
/// first two pieces (working context and working memory, appended first) are
/// never dropped, even when they alone overflow.
pub(crate) fn enforce(ctx: &mut AssembledContext, max_tokens: i64) {
    if max_tokens <= 0 {
        return;
    }
    let mut total: i64 = ctx.pieces.iter().map(|p| p.tokens).sum();
    let mut report = BudgetReport {
        max_tokens,
        estimated_tokens: total,
        dropped_pieces: 0,
        dropped_ids: Vec::new(),
        reason: String::new(),
        fit: total <= max_tokens,
    };
    if report.fit {
        "all pieces fit within the budget".clone_into(&mut report.reason);
        ctx.budget_report = Some(report);
        return;
    }
    let mut kept = ctx.pieces.len();
    while kept > 2 && total > max_tokens {
        kept -= 1;
        let piece = &ctx.pieces[kept];
        total -= piece.tokens;
        report.dropped_pieces += 1;
        report.dropped_ids.push(piece.layer.as_str().to_owned());
    }
    ctx.pieces.truncate(kept);
    report.estimated_tokens = total;
    DROP_ORDER_REASON.clone_into(&mut report.reason);
    ctx.budget_report = Some(report);
}
