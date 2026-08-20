package contextassembler

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// Working-memory-specific integration tests not already covered in assembler_test.go.
// These focus on the working-memory tier lifecycle and the Assembler's interaction with it.

func TestWorkingMemory_MultipleSessionsRespectsBudget(t *testing.T) {
	cfg := config.Defaults()
	cfg.Context.TokenBudget = 50
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 5

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	for i := 0; i < 3; i++ {
		if err := database.SaveMemory(&db.Memory{
			ID:        "wm-b-" + string(rune('a'+i)),
			Content:   "Medium length working memory content to test budget constraints across multiple items",
			Scope:     "project",
			Tier:      "working",
			ExpiresAt: &futureExpiry,
			Metadata:  map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("budget test", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if ctx.UsedTokens > ctx.Budget+50 {
		t.Errorf("used tokens (%d) exceeds budget (%d) by margin", ctx.UsedTokens, ctx.Budget)
	}
}

func TestWorkingMemory_NoWorkingMemories(t *testing.T) {
	cfg := config.Defaults()
	cfg.WorkingMemory.IncludeInContext = true

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test", "", "")
	if err != nil {
		t.Fatal(err)
	}
	// Should not panic or error with no working memories saved
	if ctx.UsedTokens < 0 {
		t.Error("expected non-negative used tokens")
	}
}

func TestAssembler_WithFullConfig(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)
	// WithFullConfig should not panic
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("full config test", "Session text here", "")
	if err != nil {
		t.Fatal(err)
	}
	if ctx == nil {
		t.Fatal("expected non-nil context")
	}
}
