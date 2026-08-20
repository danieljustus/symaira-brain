package db

import (
	"fmt"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// mustComputeLSH computes an LSH hash, failing the test on a dimension error.
func mustComputeLSH(vec []float32) int {
	h, err := ComputeLSH(vec)
	if err != nil {
		panic(fmt.Sprintf("ComputeLSH: %v", err))
	}
	return h
}

// TestComputeLSHDimensionMismatchReturnsError covers issue #439 AC2: a vector
// from a different embedding space must produce a handled error, not a silent
// truncation.
func TestComputeLSHDimensionMismatchReturnsError(t *testing.T) {
	if _, err := ComputeLSH(make([]float32, 100)); err == nil {
		t.Fatal("expected dimension mismatch error for 100-dim vector, got nil")
	}
	if _, err := ComputeLSH(make([]float32, EmbeddingDim)); err != nil {
		t.Fatalf("expected nil error for %d-dim vector, got %v", EmbeddingDim, err)
	}
}

// TestDifferentSpacesNotCrossScored covers issue #439 AC4: vectors from
// different embedding spaces (same model and dimension, different quantization
// level) must never be scored against each other.
func TestDifferentSpacesNotCrossScored(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	vec := make([]float32, EmbeddingDim)
	vec[0] = 1.0
	vec[37] = 0.5

	unquantized := &Memory{
		ID:                    "space-unquantized",
		Content:               "memory in the legacy unquantized space",
		Scope:                 "global",
		Metadata:              map[string]string{},
		Embedding:             vec,
		EmbeddingSource:       "ollama",
		EmbeddingModel:        "nomic-embed-text",
		EmbeddingQuantization: "",
	}
	quantized := &Memory{
		ID:                    "space-q4",
		Content:               "memory in the 4-bit quantized space",
		Scope:                 "global",
		Metadata:              map[string]string{},
		Embedding:             vec,
		EmbeddingSource:       "ollama",
		EmbeddingModel:        "nomic-embed-text",
		EmbeddingQuantization: "q4",
	}
	if err := database.SaveMemory(unquantized); err != nil {
		t.Fatal(err)
	}
	if err := database.SaveMemory(quantized); err != nil {
		t.Fatal(err)
	}

	// Query in the legacy space: only the unquantized memory may be scored.
	legacy, err := database.SearchMemoriesFilteredWithTrust(vec, "ollama", "global", 10, "", TrustFilter{}, PolicyFilter{}, TimeWindow{}, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(legacy) != 1 || legacy[0].Memory.ID != "space-unquantized" {
		t.Fatalf("legacy-space query: expected exactly space-unquantized, got %d results", len(legacy))
	}

	// Query in the quantized space: only the quantized memory may be scored.
	quant, err := database.SearchMemoriesFilteredWithTrust(vec, "ollama", "global", 10, "", TrustFilter{}, PolicyFilter{}, TimeWindow{}, "q4")
	if err != nil {
		t.Fatal(err)
	}
	if len(quant) != 1 || quant[0].Memory.ID != "space-q4" {
		t.Fatalf("q4-space query: expected exactly space-q4, got %d results", len(quant))
	}
}
