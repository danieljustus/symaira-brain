package security

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestLLMConsolidateMemoriesSingleEntry(t *testing.T) {
	enhancer := NewLLMEnhancer()

	memories := []*db.Memory{
		{
			ID:      "mem-1",
			Content: "User prefers TypeScript",
			Scope:   "global",
		},
	}

	facts, err := enhancer.ConsolidateMemories(memories)
	if err != nil {
		t.Fatalf("unexpected error for single memory: %v", err)
	}

	if len(facts) != 1 {
		t.Errorf("expected 1 fact, got %d", len(facts))
	}
	if facts[0] != "User prefers TypeScript" {
		t.Errorf("expected original fact, got '%s'", facts[0])
	}
}

func TestLLMConsolidateMemoriesEmptyList(t *testing.T) {
	enhancer := NewLLMEnhancer()

	facts, err := enhancer.ConsolidateMemories(nil)
	if err != nil {
		t.Fatalf("unexpected error for empty list: %v", err)
	}
	if len(facts) != 0 {
		t.Errorf("expected 0 facts for empty list, got %d", len(facts))
	}
}

func TestLLMEnhancerDefaults(t *testing.T) {
	enhancer := NewLLMEnhancer()

	if enhancer.OllamaURL == "" {
		t.Error("OllamaURL should be set by default")
	}
	if enhancer.Model == "" {
		t.Error("Model should be set by default")
	}
	if enhancer.Model != "llama3" {
		t.Errorf("expected default model 'llama3', got '%s'", enhancer.Model)
	}
}

func TestLLMConsolidateMemoriesDeduplicationLogic(t *testing.T) {
	enhancer := NewLLMEnhancer()

	memories := []*db.Memory{
		{
			ID:      "mem-1",
			Content: "User prefers Go",
			Scope:   "global",
		},
		{
			ID:      "mem-2",
			Content: "User prefers TypeScript",
			Scope:   "project",
		},
	}

	facts, err := enhancer.ConsolidateMemories(memories)
	if err != nil {
		t.Fatalf("graceful fallback should return original content without error: %v", err)
	}

	if len(facts) == 0 {
		t.Error("expected non-empty facts list from graceful fallback")
	}
}

func TestParseFallbackList(t *testing.T) {
	input := `["fact one","fact two"]`
	result := parseFallbackList(input)
	if len(result) == 0 {
		t.Error("expected non-empty result from fallback parser")
	}
}
