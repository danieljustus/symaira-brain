package contextassembler

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestAssembler_RecallReceipts(t *testing.T) {
	cfg := config.Defaults()
	cfg.Context.TokenBudget = 5000
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDegradationConfig(&cfg.Degradation)

	r := db.SearchResult{
		Memory: &db.Memory{
			ID:        "receipt-mem",
			Content:   "A deterministic receipt fact.",
			Scope:     "global",
			CreatedAt: time.Now().Add(-2 * time.Hour),
		},
		Score: 0.9,
	}
	results := []db.SearchResult{r}

	// Receipts are off by default.
	pieces := a.fillRetrievalWithDegradation(results, 5000)
	if len(pieces) == 0 {
		t.Fatal("expected a piece")
	}
	if pieces[0].Receipt != "" {
		t.Errorf("receipt should be off by default, got %q", pieces[0].Receipt)
	}

	// Enabled: the piece carries the engine-minted one-liner.
	a.WithRecallReceipts(true)
	pieces = a.fillRetrievalWithDegradation(results, 5000)
	if !strings.HasPrefix(pieces[0].Receipt, "◉ memory: ") {
		t.Errorf("expected a minted receipt, got %q", pieces[0].Receipt)
	}
	if !strings.Contains(pieces[0].Receipt, "(global, 2h)") {
		t.Errorf("receipt missing scope/age, got %q", pieces[0].Receipt)
	}
}

func TestAssembler_WithFullConfigMirrorsReceiptsFlag(t *testing.T) {
	cfg := config.Defaults()
	cfg.MCP.RecallReceipts = false
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context).WithFullConfig(cfg)
	if a.recallReceipts {
		t.Error("WithFullConfig must mirror recall_receipts=false")
	}

	cfg2 := config.Defaults()
	a2 := NewAssembler(database, nil, &cfg2.Context).WithFullConfig(cfg2)
	if !a2.recallReceipts {
		t.Error("WithFullConfig must mirror the default recall_receipts=true")
	}
}
