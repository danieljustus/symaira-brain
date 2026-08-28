package db

import (
	"fmt"
	"math"
	"testing"
	"time"

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
		emb[0] = 1.0
		emb[1] = 0.5
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

	b.Run("before_prefilter", func(b *testing.B) {
		database.prefilterEnabled = false
		database.ResetHydrationStats()
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			if _, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, ""); err != nil {
				b.Fatal(err)
			}
		}
		b.StopTimer()
		b.Logf("before: rows_hydrated=%d embedding_bytes_decoded=%d", database.HydrationStats().RowsHydrated, database.HydrationStats().EmbeddingBytesDecoded)
	})
	b.Run("after_prefilter", func(b *testing.B) {
		database.prefilterEnabled = true
		database.ResetHydrationStats()
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			if _, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, ""); err != nil {
				b.Fatal(err)
			}
		}
		b.StopTimer()
		b.Logf("after: rows_hydrated=%d embedding_bytes_decoded=%d", database.HydrationStats().RowsHydrated, database.HydrationStats().EmbeddingBytesDecoded)
	})
}

// TestPrefilter_PreservesOrderingAndScores verifies that moving the Hamming
// prefilter before full hydration does not change the returned top results or
// their scores when the fixture's true nearest neighbors are retained.
func TestPrefilter_PreservesOrderingAndScores(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	cfg.HybridSearch.QuantizeToBinary = true
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	queryVec := make([]float32, EmbeddingDim)
	for i := range queryVec {
		queryVec[i] = 1
	}
	queryLSH, err := ComputeLSH(queryVec)
	if err != nil {
		t.Fatal(err)
	}

	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	for i := 0; i < 100; i++ {
		emb := make([]float32, EmbeddingDim)
		if i < 10 {
			copy(emb, queryVec)
		} else {
			for j := range emb {
				emb[j] = 1
				if j >= EmbeddingDim/2 {
					emb[j] = 0.01
				}
			}
		}
		m := &Memory{
			ID:        fmt.Sprintf("parity-%03d", i),
			Content:   fmt.Sprintf("parity fixture %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
			CreatedAt: now.Add(-time.Duration(i) * time.Minute),
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			t.Fatalf("save %d: %v", i, err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}
	if _, err := database.conn.Exec("UPDATE memories SET lsh_hash = ?", queryLSH); err != nil {
		t.Fatal(err)
	}

	weights := RankingWeights{RelevanceWeight: 1}
	database.prefilterEnabled = false
	database.ResetHydrationStats()
	offResults, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, "", weights)
	if err != nil {
		t.Fatalf("search (prefilter off): %v", err)
	}
	offMetrics := database.HydrationStats()

	database.prefilterEnabled = true
	database.ResetHydrationStats()
	onResults, err := database.SearchMemoriesFiltered(queryVec, "", "global", 10, "", weights)
	if err != nil {
		t.Fatalf("search (prefilter on): %v", err)
	}
	onMetrics := database.HydrationStats()

	if len(offResults) != len(onResults) {
		t.Fatalf("result count differs: off=%d on=%d", len(offResults), len(onResults))
	}
	for i := range offResults {
		if offResults[i].Memory.ID != onResults[i].Memory.ID {
			t.Errorf("rank %d: ID mismatch off=%s on=%s", i, offResults[i].Memory.ID, onResults[i].Memory.ID)
		}
		if diff := math.Abs(float64(offResults[i].Score - onResults[i].Score)); diff > 1e-6 {
			t.Errorf("rank %d: score mismatch off=%.8f on=%.8f diff=%.8f", i, offResults[i].Score, onResults[i].Score, diff)
		}
	}
	if offMetrics.RowsHydrated != 100 || offMetrics.EmbeddingBytesDecoded != 100*EmbeddingDim*4 {
		t.Fatalf("unexpected before metrics: %+v", offMetrics)
	}
	if onMetrics.RowsHydrated != 64 || onMetrics.EmbeddingBytesDecoded != 64*EmbeddingDim*4 {
		t.Fatalf("unexpected after metrics: %+v", onMetrics)
	}
}

func resultIDSet(rs []SearchResult) map[string]bool {
	s := make(map[string]bool, len(rs))
	for _, r := range rs {
		s[r.Memory.ID] = true
	}
	return s
}
