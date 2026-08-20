package contextassembler

import (
	"strings"
	"unicode/utf8"
)

// TokenEstimator estimates the token count of a piece of text. Callers
// with a real tokenizer can plug one in via Assembler.WithTokenEstimator
// (#492); the default is a conservative characters/4 heuristic.
type TokenEstimator func(text string) int

// DefaultTokenEstimator estimates tokens as runes/4 + 1 per line — the
// long-standing heuristic of this package (chars-per-token ~4). It
// over-estimates for code-heavy text, which is the safe direction for a
// budget guard.
func DefaultTokenEstimator(text string) int {
	if text == "" {
		return 0
	}
	tokens := utf8.RuneCountInString(text)/4 + 1
	// Newlines are meaningful tokens in code; count them explicitly.
	tokens += strings.Count(text, "\n")
	return tokens
}

// BudgetReport describes what a hard token budget did to an assembly
// (#492): whether it was enforced, what was dropped and why. It lets
// callers show the user a truthful "what did I leave out" answer instead
// of silently trimming.
type BudgetReport struct {
	// MaxTokens is the enforced ceiling (0 = no budget, additive mode).
	MaxTokens int `json:"max_tokens"`
	// EstimatedTokens is the final estimated total of the assembled pieces.
	EstimatedTokens int `json:"estimated_tokens"`
	// DroppedPieces is how many assembled pieces were dropped.
	DroppedPieces int `json:"dropped_pieces"`
	// DroppedIDs names the dropped pieces (layer + optional memory ids).
	DroppedIDs []string `json:"dropped_ids,omitempty"`
	// Reason explains the drop order that was applied.
	Reason string `json:"reason,omitempty"`
	// Fit is true when everything assembled fit within the budget.
	Fit bool `json:"fit"`
}

// dropOrderReason documents the fixed drop order applied when a budget is
// exceeded (#492): working context and working memory are never dropped;
// everything else is dropped lowest-priority first (retrieval before
// session context before profile before summary).
const dropOrderReason = "hard budget: dropped lowest-priority pieces first; working context and working memory are never dropped"

// enforceBudget trims ctx.Pieces so their estimated total fits under
// maxTokens and fills ctx.Budget. Pieces are appended in priority order
// (working context, working memory, summary, profile, session context,
// retrieval), so dropping from the end naturally keeps the pinned/working
// set and drops lowest-ranked retrieval content first. Callers that want
// today's exact behaviour leave the budget at 0.
func (a *Assembler) enforceBudget(ctx *AssembledContext, maxTokens int) {
	if maxTokens <= 0 {
		return
	}
	report := &BudgetReport{MaxTokens: maxTokens}
	total := 0
	for i := range ctx.Pieces {
		total += ctx.Pieces[i].Tokens
	}
	report.EstimatedTokens = total
	report.Fit = total <= maxTokens
	ctx.BudgetReport = report
	if report.Fit {
		report.Reason = "all pieces fit within the budget"
		return
	}

	// Drop from the end (lowest priority) until the ceiling is met. The
	// first two layers are the pinned/working set and are never dropped —
	// they are appended first, so a reverse walk naturally preserves them
	// as long as they alone fit; if even they overflow, nothing can be
	// done and we keep them anyway (they are the mandated minimum).
	kept := len(ctx.Pieces)
	for kept > 2 && total > maxTokens {
		kept--
		piece := ctx.Pieces[kept]
		total -= piece.Tokens
		report.DroppedPieces++
		report.DroppedIDs = append(report.DroppedIDs, string(piece.Layer))
	}
	ctx.Pieces = ctx.Pieces[:kept]
	report.EstimatedTokens = total
	report.Reason = dropOrderReason
}
