package db

import (
	"fmt"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// TestPrefilter_MixedBinaryEquivalence verifies that a candidate set with
// mixed nil/non-nil EmbeddingBinary returns the same memories with the
// prefilter enabled and disabled. The prefilter must SKIP (not drop) when any
// candidate lacks a binary vector, so pre-quantization memories survive.
//
// Regression test for #534.
func TestPrefilter_MixedBinaryEquivalence(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	// Phase 1: quantize off — memories saved WITHOUT binary vectors
	// (the normal upgrade case: a store written before quantize_to_binary).
	database.quantizeBinary = false
	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 20; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		m := &Memory{
			ID:        fmt.Sprintf("legacy-%02d", i),
			Content:   fmt.Sprintf("legacy memory number %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			t.Fatalf("save legacy %d: %v", i, err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}

	// Phase 2: quantize on — memories saved WITH binary vectors.
	database.quantizeBinary = true
	tx2, err := database.BeginTransaction()
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 20; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		m := &Memory{
			ID:        fmt.Sprintf("quantized-%02d", i),
			Content:   fmt.Sprintf("quantized memory number %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if err := database.SaveMemoryTx(tx2, m); err != nil {
			t.Fatalf("save quantized %d: %v", i, err)
		}
	}
	if err := tx2.Commit(); err != nil {
		t.Fatal(err)
	}

	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0

	// Prefilter OFF.
	database.prefilterEnabled = false
	offResults, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, "")
	if err != nil {
		t.Fatalf("search (prefilter off): %v", err)
	}

	// Prefilter ON — must return the same memories (skip, not drop).
	database.prefilterEnabled = true
	onResults, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, "")
	if err != nil {
		t.Fatalf("search (prefilter on): %v", err)
	}

	offIDs := resultIDSet(offResults)
	onIDs := resultIDSet(onResults)
	if len(offIDs) != len(onIDs) {
		t.Fatalf("result count differs: off=%d on=%d", len(offIDs), len(onIDs))
	}
	for id := range offIDs {
		if !onIDs[id] {
			t.Errorf("memory %q present with prefilter off but missing with prefilter on", id)
		}
	}
}

// BenchmarkPrefilter_OnVsOff records the prefilter on/off difference so a
// future no-op regression (prefilter returning every candidate) is visible.
func BenchmarkPrefilter_OnVsOff(b *testing.B) {
	cfg := config.Defaults()
	cfg.Database.Path = b.TempDir() + "/test.db"
	cfg.HybridSearch.QuantizeToBinary = true
	database, err := Open(cfg)
	if err != nil {
		b.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	tx, err := database.BeginTransaction()
	if err != nil {
		b.Fatal(err)
	}
	for i := 0; i < 1000; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		emb[(i+37)%EmbeddingDim] = 0.5
		m := &Memory{
			ID:        fmt.Sprintf("bench-%04d", i),
			Content:   fmt.Sprintf("benchmark memory number %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			b.Fatalf("save %d: %v", i, err)
		}
	}
	if err := tx.Commit(); err != nil {
		b.Fatal(err)
	}

	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0

	b.Run("off", func(b *testing.B) {
		database.prefilterEnabled = false
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			if _, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, ""); err != nil {
				b.Fatal(err)
			}
		}
	})
	b.Run("on", func(b *testing.B) {
		database.prefilterEnabled = true
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			if _, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, ""); err != nil {
				b.Fatal(err)
			}
		}
	})
}

func resultIDSet(rs []SearchResult) map[string]bool {
	s := make(map[string]bool, len(rs))
	for _, r := range rs {
		s[r.Memory.ID] = true
	}
	return s
}
