package contextassembler

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// ---------------------------------------------------------------------------
// #492: hard token budget with pluggable estimator and drop report
// ---------------------------------------------------------------------------

func TestDefaultTokenEstimator(t *testing.T) {
	if n := DefaultTokenEstimator(""); n != 0 {
		t.Fatalf("empty text = %d tokens, want 0", n)
	}
	if n := DefaultTokenEstimator(strings.Repeat("a", 400)); n != 101 {
		t.Fatalf("400 chars = %d tokens, want 101 (runes/4 + 1)", n)
	}
	// Newlines count as tokens (code-aware direction).
	if n := DefaultTokenEstimator("a\nb\nc\n"); n <= DefaultTokenEstimator("abc") {
		t.Fatalf("newlines must add tokens: %d vs %d", n, DefaultTokenEstimator("abc"))
	}
}

func TestEnforceBudgetDropOrder(t *testing.T) {
	a := NewAssembler(nil, nil, &config.Defaults().Context)

	// Pieces are appended in priority order: working context, working
	// memory, summary, retrieval. The budget must drop retrieval first and
	// never the working set.
	ctx := &AssembledContext{
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "session", Tokens: 20},
			{Layer: LayerWorkingMemory, Content: "wm", Tokens: 20},
			{Layer: LayerSummary, Content: "summary", Tokens: 20},
			{Layer: LayerRetrieval, Content: "retrieval", Tokens: 20},
		},
	}
	a.enforceBudget(ctx, 65)
	if ctx.BudgetReport == nil {
		t.Fatal("budget report missing")
	}
	if ctx.BudgetReport.Fit {
		t.Fatalf("65 token budget with 80 tokens total must not fit")
	}
	if ctx.BudgetReport.DroppedPieces != 1 {
		t.Fatalf("dropped %d pieces, want 1", ctx.BudgetReport.DroppedPieces)
	}
	if len(ctx.Pieces) != 3 {
		t.Fatalf("kept %d pieces, want 3", len(ctx.Pieces))
	}
	if ctx.Pieces[2].Layer != LayerSummary {
		t.Fatalf("retrieval must be dropped first, kept %v", ctx.Pieces[2].Layer)
	}
	if ctx.Pieces[0].Layer != LayerWorkingContext || ctx.Pieces[1].Layer != LayerWorkingMemory {
		t.Fatalf("working set must never be dropped: %v", ctx.Pieces)
	}
	if len(ctx.BudgetReport.DroppedIDs) != 1 || ctx.BudgetReport.DroppedIDs[0] != string(LayerRetrieval) {
		t.Fatalf("dropped ids = %v, want [retrieval]", ctx.BudgetReport.DroppedIDs)
	}

	// Everything fits → report says so, nothing dropped.
	ctx2 := &AssembledContext{Pieces: []AssembledPiece{{Layer: LayerWorkingContext, Content: "s", Tokens: 10}}}
	a.enforceBudget(ctx2, 100)
	if !ctx2.BudgetReport.Fit || ctx2.BudgetReport.DroppedPieces != 0 {
		t.Fatalf("fit case report wrong: %+v", ctx2.BudgetReport)
	}

	// No budget → additive mode: no report, no change.
	ctx3 := &AssembledContext{Pieces: []AssembledPiece{{Layer: LayerRetrieval, Content: "x", Tokens: 500}}}
	a.enforceBudget(ctx3, 0)
	if ctx3.BudgetReport != nil {
		t.Fatalf("no budget must not produce a report")
	}
	if len(ctx3.Pieces) != 1 {
		t.Fatalf("no budget must not drop pieces")
	}
}

func TestAssemblerPluggableEstimator(t *testing.T) {
	a := NewAssembler(nil, nil, &config.Defaults().Context)
	if n := a.estimate("hello world foo"); n != DefaultTokenEstimator("hello world foo") {
		t.Fatalf("default estimator mismatch: %d", n)
	}

	called := false
	a.WithTokenEstimator(func(text string) int {
		called = true
		return len(text) * 10
	})
	if n := a.estimate("hi"); n != 20 {
		t.Fatalf("pluggable estimator not used: %d", n)
	}
	if !called {
		t.Fatal("estimator was not invoked")
	}
}

// TestAssembleProducesBudgetReportWithRealAssembler wires the estimator
// through a full assembly and verifies the report lands on the context.
func TestAssembleProducesBudgetReportWithRealAssembler(t *testing.T) {
	cfg := config.Defaults()
	cfg.Context.TokenBudget = 40
	a := NewAssembler(nil, nil, &cfg.Context)
	ctx, err := a.Assemble("query", strings.Repeat("session text ", 20), "")
	if err != nil {
		t.Fatal(err)
	}
	if ctx.BudgetReport == nil {
		t.Fatal("assembled context must carry a budget report")
	}
	if ctx.BudgetReport.MaxTokens != 40 {
		t.Fatalf("report max = %d, want 40", ctx.BudgetReport.MaxTokens)
	}
	if !ctx.BudgetReport.Fit {
		// Some pieces fit, some were dropped; the report must reflect it.
		t.Logf("budget report: %+v", ctx.BudgetReport)
	}
}
